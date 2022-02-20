use crate::util::{json_response, SingleMainLua};
use crate::{MainState, Result};
use hive_asar::FileArchive;
use hive_core::permission::PermissionSet;
use hive_core::{ErrorKind, Source};
use hyper::{Body, Request, Response, StatusCode};
use log::info;
use multer::{Constraints, Multipart, SizeLimit};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::fs;

#[derive(Debug, Serialize, Deserialize)]
struct UploadQuery {
  #[serde(rename = "type")]
  kind: UploadType,
}

#[derive(Debug, Serialize, Deserialize)]
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
  let name_provided = name.is_some();
  let (parts, body) = req.into_parts();

  let content_type = (parts.headers)
    .get("Content-Type")
    .ok_or("no Content-Type given")?
    .to_str()
    .or(Err("Content-Type is not valid UTF-8"))?;
  let boundary = multer::parse_boundary(content_type)?;
  let constraints = Constraints::new()
    .allowed_fields(vec!["source"])
    .size_limit(SizeLimit::new().for_field("source", 1024u64.pow(3)));
  let mut multipart = Multipart::with_constraints(body, boundary, constraints);

  // TODO: Add permissions

  // should be exactly one field, so a single `.next_field` is probably OK
  let source_field = multipart
    .next_field()
    .await?
    .ok_or("no source code uploaded")?;
  let name = name
    .or_else(|| {
      source_field.file_name().map(|x| {
        let (x, _) = x.rsplit_once('.').unwrap_or((x, ""));
        slug::slugify(x).into()
      })
    })
    .ok_or("no service name provided")?;

  let query = parts.uri.query().unwrap_or("");
  let UploadQuery { kind } = serde_qs::from_str(query)?;

  if !name_provided && state.hive.get_service(&name).await.is_ok() {
    return Err((409, "service already exists", json!({ "name": name })).into());
  }

  let replaced = match state.hive.remove_service(&name).await {
    Ok(replaced) => Some(replaced),
    Err(error) if matches!(error.kind(), ErrorKind::ServiceNotFound(_)) => None,
    Err(error) => return Err(error.into()),
  };

  let result = async {
    let source_path = state.config_path.join(format!("services/{}", name));
    if source_path.exists() {
      fs::remove_dir_all(&source_path).await?;
    }
    fs::create_dir(&source_path).await?;

    let source_bytes = source_field.bytes().await?;
    let source = match kind {
      UploadType::Single => {
        fs::write(source_path.join("main.lua"), &source_bytes).await?;
        let vfs = SingleMainLua::from_slice(source_bytes);
        Source::new(vfs)
      }
      UploadType::Multi => {
        let path = source_path.join("main.asar");
        fs::write(&path, &source_bytes).await?;
        let vfs = FileArchive::new(path)
          .await
          .map_err(|error| (400, "error parsing ASAR archive", error.to_string()))?;
        Source::new(vfs)
      }
    };

    let service = state
      .hive
      .create_service(name, source, PermissionSet::new())
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
