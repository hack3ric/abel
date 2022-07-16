use crate::server::error::{Error, ErrorKind};
use crate::server::metadata::Metadata;
use crate::server::source::{AsarSource, SingleSource};
use crate::server::{json_response, Result, ServerState};
use abel_core::service::{ErrorPayload, Service};
use abel_core::ErrorKind::ServiceExists;
use abel_core::{Config, ServiceImpl};
use abel_rt::Source;
use futures::TryStreamExt;
use hive_asar::Archive;
use hyper::{Body, HeaderMap, Request, Response, StatusCode};
use log::{info, warn};
use multer::{Constraints, Multipart, SizeLimit};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt};
use tokio_util::io::StreamReader;
use uuid::Uuid;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum UploadMode {
  #[default]
  #[serde(rename = "create")]
  Create,

  #[serde(rename = "hot")]
  Hot,

  #[serde(rename = "cold")]
  Cold,

  #[serde(rename = "load")]
  Load,
}

#[derive(Deserialize)]
struct UploadQuery {
  #[serde(default)]
  mode: UploadMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
  Single,
  Multi,
}

pub(crate) async fn upload(
  state: &ServerState,
  name: String,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let (parts, body) = req.into_parts();
  let mut multipart = parse_multipart(&parts.headers, body)?;

  let UploadQuery { mode } = serde_qs::from_str(parts.uri.query().unwrap_or(""))?;

  let source_field = multipart.next_field().await?.ok_or((
    "no source uploaded",
    "specify either `single` or `multi` field in multipart",
  ))?;

  let temp_path = state.abel_path.join(format!("tmp/{}", Uuid::new_v4()));

  let (kind, source, config): (_, Source, Config) = match source_field.name() {
    Some("single") => {
      let code = source_field.bytes().await?;
      fs::write(&temp_path, &code).await?;

      let source = Source::new(SingleSource::new(code));
      (SourceKind::Single, source, Default::default())
    }
    Some("multi") => {
      let mut reader =
        StreamReader::new(source_field.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
      let mut writer = File::create(&temp_path).await?;
      io::copy(&mut reader, &mut writer).await?;

      let mut archive = Archive::new_from_file(&temp_path).await?;

      let config = if let Ok(mut config_file) = archive.get("abel.json").await {
        let mut config_bytes = vec![0; config_file.metadata().size as _];
        config_file.read_to_end(&mut config_bytes).await?;
        serde_json::from_slice(&config_bytes)?
      } else {
        Default::default()
      };

      let source = Source::new(AsarSource(archive));
      (SourceKind::Multi, source, config)
    }
    _ => {
      return Err(From::from((
        "unknown field name",
        "first field is neither named `single` nor `multi`",
      )))
    }
  };

  let (service, replaced, error_payload) =
    create_service(state, mode, name, config, source, kind, &temp_path).await?;

  response(service, replaced, error_payload).await
}

fn parse_multipart(headers: &HeaderMap, body: Body) -> Result<Multipart<'static>> {
  let allowed_fields = vec!["single", "multi", "config"];
  let size_limit = SizeLimit::new()
    .for_field("single", 1024u64.pow(2) * 5)
    .for_field("multi", 1024u64.pow(2) * 100)
    .for_field("config", 1024u64.pow(2) * 5);

  let content_type = headers
    .get("content-type")
    .ok_or("no Content-Type given")?
    .to_str()
    .or(Err("Content-Type is not valid UTF-8"))?;
  let boundary = multer::parse_boundary(content_type)?;
  let constraints = Constraints::new()
    .allowed_fields(allowed_fields)
    .size_limit(size_limit);
  Ok(Multipart::with_constraints(body, boundary, constraints))
}

async fn create_service<'a>(
  state: &'a ServerState,
  mode: UploadMode,
  name: String,
  config: Config,
  source: Source,
  source_kind: SourceKind,
  temp_path: &Path,
) -> Result<(Service<'a>, Option<ServiceImpl>, ErrorPayload)> {
  let (service, replaced, error_payload) = match mode {
    UploadMode::Create if state.abel.get_service(&name).is_ok() => {
      return Err(ServiceExists { name: name.into() }.into())
    }
    UploadMode::Hot if state.abel.get_running_service(&name).is_ok() => {
      let (service, replaced) = (state.abel)
        .hot_update_service(name, None, source, config)
        .await?;
      (
        Service::Running(service),
        Some(replaced),
        Default::default(),
      )
    }
    UploadMode::Hot | UploadMode::Cold | UploadMode::Create => {
      (state.abel)
        .cold_update_or_create_service(name, None, source, config)
        .await?
    }
    UploadMode::Load => {
      let (service, replaced, error_payload) = (state.abel)
        .load_service(name, None, source, config)
        .await?;
      (Service::Stopped(service), replaced, error_payload)
    }
  };
  let guard = service.upgrade();

  let service_path = state.abel_path.join("services").join(guard.name());
  if service_path.exists() {
    fs::remove_dir_all(&service_path).await?;
  }
  fs::create_dir(&service_path).await?;

  let metadata = Metadata {
    uuid: guard.uuid(),
    started: true,
  };
  fs::write(
    service_path.join("metadata.json"),
    serde_json::to_string(&metadata)?,
  )
  .await?;

  match source_kind {
    SourceKind::Single => fs::rename(temp_path, service_path.join("source.lua")).await?,
    SourceKind::Multi => fs::hard_link(temp_path, service_path.join("source.asar")).await?,
  }

  Ok((service, replaced, error_payload))
}

async fn response(
  service: Service<'_>,
  replaced: Option<ServiceImpl>,
  error_payload: ErrorPayload,
) -> Result<Response<Body>> {
  let service = service.upgrade();
  let mut body = serde_json::Map::<String, serde_json::Value>::new();
  body.insert("new_service".into(), json!(service));

  if let Some(replaced) = replaced {
    info!(
      "Updated service '{}' {}",
      service.name(),
      format!("({} -> {})", replaced.uuid(), service.uuid()).dimmed(),
    );
    body.insert("replaced_service".into(), json!(replaced));
  } else {
    info!(
      "Created service '{}' {}",
      service.name(),
      format!("({})", service.uuid()).dimmed(),
    );
  }

  if !error_payload.is_empty() {
    warn!("error payload: {error_payload:?}");
    let mut map = serde_json::Map::<String, serde_json::Value>::new();
    if let Some(stop) = error_payload.stop {
      map.insert(
        "stop".into(),
        json!(Error::from(ErrorKind::Abel(stop)).into_status_and_body().1),
      );
    }
    if let Some(start) = error_payload.start {
      map.insert(
        "start".into(),
        json!(Error::from(ErrorKind::Abel(start)).into_status_and_body().1),
      );
    }
    body.insert("errors".into(), serde_json::Value::Object(map));
  }

  json_response(StatusCode::OK, body)
}
