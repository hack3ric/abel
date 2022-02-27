use crate::util::json_response;
use crate::{MainState, Result};
use futures::TryStreamExt;
use hive_asar::Archive;
use hive_core::permission::PermissionSet;
use hive_core::{ErrorKind, Source};
use hyper::{Body, Request, Response, StatusCode};
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

/// ```plaintext
/// POST /services (slugified filename as service name)
/// PUT /services/{name}
/// ```
pub(crate) async fn upload(
  state: &MainState,
  mut name: Option<String>,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let (parts, body) = req.into_parts();

  let qs = parts.uri.query().unwrap_or("");
  let UploadQuery { kind } = serde_qs::from_str(qs)?;

  let content_type = (parts.headers)
    .get("Content-Type")
    .ok_or("no Content-Type given")?
    .to_str()
    .or(Err("Content-Type is not valid UTF-8"))?;
  let boundary = multer::parse_boundary(content_type)?;
  let constraints = Constraints::new()
    .allowed_fields(vec!["source", "permissions"])
    .size_limit(
      SizeLimit::new()
        .for_field("source", 1024u64.pow(2) * 100)
        .for_field("permissions", 1024u64 * 5),
    );
  let mut multipart = Multipart::with_constraints(body, boundary, constraints);

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
        permissions_result = Some(parse_permissions(&field.bytes().await?).await?);
      }
      _ => unreachable!(),
    }
  }

  let (name, file, path) = source_result.ok_or("no source code uploaded")?;

  let replaced = match state.hive.remove_service(&name).await {
    Ok(replaced) => Some(replaced),
    Err(error) if matches!(error.kind(), ErrorKind::ServiceNotFound(_)) => None,
    Err(error) => return Err(error.into()),
  };
  let result = async {
    let source = parse_source(state, kind, &name, file, path).await?;

    let permissions = if kind == UploadType::Single {
      permissions_result.unwrap_or_else(PermissionSet::new)
    } else {
      match source.get_bytes("/permissions.json").await {
        Ok(bytes) => parse_permissions(&bytes).await?,
        Err(error) => {
          if let ErrorKind::Io(io_error) = error.kind() {
            if let tokio::io::ErrorKind::NotFound = io_error.kind() {
              permissions_result.unwrap_or_else(PermissionSet::new)
            } else {
              return Err(error.into());
            }
          } else {
            return Err(error.into());
          }
        }
      }
    };

    let service = (state.hive)
      .create_service(name, source, permissions)
      .await?;
    Ok::<_, crate::error::Error>(service)
  }
  .await;

  let service = match result {
    Ok(service) => service,
    Err(mut error) => {
      error.add_detail(
        "replaced_service".to_string(),
        serde_json::to_value(replaced)?,
      );
      return Err(error);
    }
  };
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
        slug::slugify(x).into()
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

async fn parse_source(
  state: &MainState,
  kind: UploadType,
  name: &str,
  mut file: fs::File,
  path: PathBuf,
) -> Result<Source> {
  let source_path = state.config_path.join(format!("services/{name}"));
  if source_path.exists() {
    fs::remove_dir_all(&source_path).await?;
  }
  fs::create_dir(&source_path).await?;

  file.seek(SeekFrom::Start(0)).await?;
  match kind {
    UploadType::Single => {
      fs::rename(path, source_path.join("main.lua")).await?;
      Ok(Source::new(source_path).await?)
    }
    UploadType::Multi => {
      let mut archive = Archive::new(file)
        .await
        .map_err(|error| (400, "error parsing ASAR archive", error.to_string()))?;
      archive.extract(&source_path).await?;
      Ok(Source::new(source_path).await?)
    }
  }
}

async fn parse_permissions(bytes: &[u8]) -> Result<PermissionSet> {
  let permissions = serde_json::from_slice::<PermissionSet>(bytes)?;
  Ok(permissions)
}
