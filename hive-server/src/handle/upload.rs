use crate::util::json_response;
use crate::{MainState, Result};
use futures::{Future, TryStreamExt};
use hive_asar::Archive;
use hive_core::permission::PermissionSet;
use hive_core::{ErrorKind, Service, ServiceGuard, Source};
use hyper::{Body, HeaderMap, Request, Response, StatusCode};
use log::info;
use multer::{Constraints, Field, Multipart, SizeLimit};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::SeekFrom;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::io::AsyncSeekExt;
use tokio::{fs, io};
use tokio_util::io::StreamReader;

#[derive(Debug, Serialize, Deserialize)]
struct UploadQuery {
  #[serde(rename = "type")]
  kind: UploadType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum UploadType {
  #[serde(rename = "single")]
  Single,
  #[serde(rename = "multi")]
  Multi,
}

pub(crate) async fn upload(
  state: &MainState,
  name: Option<String>,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let (parts, body) = req.into_parts();
  let qs = parts.uri.query().unwrap_or("");
  let UploadQuery { kind } = serde_qs::from_str(qs)
    .map_err(|error| (400, "failed to parse query string", error.to_string()))?;

  let multipart = parse_multipart(kind, &parts.headers, body)?;
  let (service, replaced) = match kind {
    UploadType::Single => upload_single(state, name, multipart).await?,
    UploadType::Multi => upload_multi(state, name, multipart).await?,
  };
  response(service, replaced).await
}

fn parse_multipart(
  kind: UploadType,
  headers: &HeaderMap,
  body: Body,
) -> Result<Multipart<'static>> {
  let mut allowed_fields = vec!["source"];
  let mut size_limit = SizeLimit::new().for_field("source", 1024u64.pow(2) * 100);
  if let UploadType::Single = kind {
    allowed_fields.push("permissions");
    size_limit = size_limit.for_field("permissions", 1024u64 * 5);
  }

  let content_type = headers
    .get("Content-Type")
    .ok_or("no Content-Type given")?
    .to_str()
    .or(Err("Content-Type is not valid UTF-8"))?;
  let boundary = multer::parse_boundary(content_type)?;
  let constraints = Constraints::new()
    .allowed_fields(allowed_fields)
    .size_limit(size_limit);
  Ok(Multipart::with_constraints(body, boundary, constraints))
}

async fn upload_single<'a>(
  state: &'a MainState,
  mut name: Option<String>,
  mut multipart: Multipart<'static>,
) -> Result<(Service, Option<ServiceGuard<'a>>)> {
  let mut source_result = None::<(String, fs::File, PathBuf)>;
  let mut permissions_result = None::<PermissionSet>;

  while let Some(field) = multipart.next_field().await? {
    match field.name() {
      Some("source") => {
        if source_result.is_some() {
          return Err("multiple sources uploaded".into());
        }
        source_result = Some(read_source(state, field, name.take()).await?);
      }
      Some("permissions") => {
        if permissions_result.is_some() {
          return Err("multiple permission sets uploaded".into());
        }
        let bytes = &field.bytes().await?;
        permissions_result = Some(serde_json::from_slice(bytes)?);
      }
      _ => unreachable!(),
    }
  }

  let (name, _, path) = source_result.ok_or("no source code uploaded")?;
  let permissions = permissions_result.unwrap_or_else(PermissionSet::new);

  service_scope(state, name, move |source_path| async move {
    fs::rename(path, source_path.join("main.lua")).await?;
    let source = Source::new(source_path).await?;
    Ok((source, permissions))
  })
  .await
}

async fn upload_multi<'a>(
  state: &'a MainState,
  name: Option<String>,
  mut multipart: Multipart<'static>,
) -> Result<(Service, Option<ServiceGuard<'a>>)> {
  let field = multipart.next_field().await?.ok_or("no source field")?;
  let (name, mut file, path) = read_source(state, field, name).await?;

  service_scope(state, name, |source_path| async move {
    file.seek(SeekFrom::Start(0)).await?;
    let mut archive = Archive::new(file)
      .await
      .map_err(|error| (400, "error parsing asar archive", error.to_string()))?;
    archive.extract(&source_path).await?;
    // drop(archive);
    fs::remove_file(path).await?;
    let source = Source::new(&source_path).await?;
    let permissions = match source.get_bytes("/permissions.json").await {
      Ok(bytes) => serde_json::from_slice(&bytes)?,
      Err(error) => {
        if let ErrorKind::Io(io_error) = error.kind() {
          if let tokio::io::ErrorKind::NotFound = io_error.kind() {
            PermissionSet::new()
          } else {
            return Err(error.into());
          }
        } else {
          return Err(error.into());
        }
      }
    };
    Ok((source, permissions))
  })
  .await
}

async fn response(service: Service, replaced: Option<ServiceGuard<'_>>) -> Result<Response<Body>> {
  let service = service.upgrade();
  if let Some(replaced) = replaced {
    info!(
      "Updated service '{}' ({} -> {})",
      service.name(),
      replaced.uuid(),
      service.uuid()
    );
    json_response(
      StatusCode::OK,
      json!({
        "new_service": service,
        "replaced_service": replaced
      }),
    )
  } else {
    info!("Created service '{}' ({})", service.name(), service.uuid());
    json_response(StatusCode::OK, json!({ "new_service": service }))
  }
}

/// Stores source to a tempfile
// TODO: get length and hint operations afterwards
async fn read_source(
  state: &MainState,
  field: Field<'_>,
  name: Option<String>,
) -> Result<(String, fs::File, PathBuf)> {
  let name_provided = name.is_some();
  let name = name
    .or_else(|| {
      field.file_name().map(|x| {
        let x = x.rsplit_once('.').map(|x| x.0).unwrap_or(x);
        slug::slugify(x)
      })
    })
    .ok_or("no service name provided")?;

  if !name_provided && state.hive.get_service(&name).await.is_ok() {
    return Err((409, "service already exists", json!({ "name": name })).into());
  }

  let (file, path) = NamedTempFile::new()?.keep().map_err(io::Error::from)?;
  let mut file = fs::File::from_std(file);

  let mut field =
    StreamReader::new(field.map_err(|error| io::Error::new(io::ErrorKind::Other, error)));
  io::copy(&mut field, &mut file).await?;

  Ok((name, file, path))
}

async fn replace_service<'a>(state: &'a MainState, name: &str) -> Result<Option<ServiceGuard<'a>>> {
  match state.hive.remove_service(name).await {
    Ok(replaced) => Ok(Some(replaced)),
    Err(error) if matches!(error.kind(), ErrorKind::ServiceNotFound(_)) => Ok(None),
    Err(error) => Err(error.into()),
  }
}

// Currently cold reloading
async fn service_scope<F, Fut>(
  state: &MainState,
  name: String,
  f: F,
) -> Result<(Service, Option<ServiceGuard<'_>>)>
where
  F: FnOnce(PathBuf) -> Fut,
  Fut: Future<Output = Result<(Source, PermissionSet)>> + Send,
{
  let temp_path = tempfile::tempdir()?.into_path();
  let source_path = state.config_path.join(format!("services/{name}"));

  let replaced = replace_service(state, &name).await?;
  let result = async {
    let (source, permissions) = f(temp_path.clone()).await?;
    let service = (state.hive)
      .create_service(name, source.clone(), permissions)
      .await?;
    if source_path.exists() {
      fs::remove_dir_all(&source_path).await?;
    }
    // fs::create_dir(&source_path).await?;
    source.rename_base(source_path.clone()).await?;
    Ok::<_, crate::error::Error>(service)
  }
  .await;
  let result = match result {
    Ok(service) => Ok((service, replaced)),
    Err(mut error) => {
      error.add_detail(
        "replaced_service".to_string(),
        serde_json::to_value(replaced)?,
      );
      Err(error)
    }
  };
  let _ = fs::remove_dir_all(temp_path).await;
  result
}
