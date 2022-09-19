use super::metadata::Metadata;
use super::types::{HttpUploadResponse, ServiceWithStatus};
use super::{json_response, Result, ServerState};
use crate::source::{AsarSource, SingleSource};
use crate::SourceKind;
use abel_core::service::{ErrorPayload, Service};
use abel_core::source::Source;
use abel_core::ErrorKind::ServiceExists;
use abel_core::{Config, ServiceImpl};
use bytes::{Bytes, BytesMut};
use futures::{Stream, TryStreamExt};
use hive_asar::Archive;
use hyper::{Body, HeaderMap, Request, Response, StatusCode};
use log::{info, warn};
use multer::{Constraints, Multipart, SizeLimit};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use strum::{Display, EnumString, IntoStaticStr};
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt};
use tokio_util::io::StreamReader;
use uuid::Uuid;

#[derive(
  Debug,
  Display,
  Default,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Serialize,
  Deserialize,
  EnumString,
  IntoStaticStr,
  clap::ValueEnum,
)]
pub enum UploadMode {
  #[default]
  #[serde(rename = "create")]
  #[strum(serialize = "create")]
  Create,
  #[serde(rename = "hot")]
  #[strum(serialize = "hot")]
  Hot,
  #[serde(rename = "cold")]
  #[strum(serialize = "cold")]
  Cold,
  #[serde(rename = "load")]
  #[strum(serialize = "load")]
  Load,
}

#[derive(Serialize, Deserialize)]
struct UploadQuery {
  #[serde(default)]
  mode: UploadMode,
}

pub struct UploadResponse<'a> {
  pub new_service: Service<'a>,
  pub replaced_service: Option<ServiceImpl>,
  pub errors: ErrorPayload,
}

pub async fn upload(
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

  let kind = match source_field.name() {
    Some("single") => SourceKind::Single,
    Some("multi") => SourceKind::Multi,
    _ => {
      return Err(From::from((
        "unknown field name",
        "first field is neither named `single` nor `multi`",
      )))
    }
  };

  let source_stream = source_field.map_err(|e| io::Error::new(io::ErrorKind::Other, e));
  let resp = upload_local(state, name, mode, kind, source_stream).await?;

  response(resp).await
}

pub async fn upload_local(
  state: &ServerState,
  name: String,
  mode: UploadMode,
  kind: SourceKind,
  source_stream: impl Stream<Item = io::Result<Bytes>> + Unpin,
) -> Result<UploadResponse> {
  let (temp_path, source, config) =
    read_store_service_temp(&state.abel_path, kind, source_stream).await?;
  create_service(state, mode, name, config, source, kind, &temp_path).await
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

async fn read_store_service_temp(
  abel_path: &Path,
  kind: SourceKind,
  mut source_stream: impl Stream<Item = io::Result<Bytes>> + Unpin,
) -> Result<(PathBuf, Source, Config)> {
  let temp_path = abel_path.join(format!("tmp/{}", Uuid::new_v4()));

  let (source, config) = match kind {
    SourceKind::Single => {
      let mut code = BytesMut::new();
      while let Some(chunk) = source_stream.try_next().await? {
        code.extend(chunk);
      }
      fs::write(&temp_path, &code).await?;

      let source = Source::new(SingleSource::new(code));
      (source, Default::default())
    }
    SourceKind::Multi => {
      let mut reader = StreamReader::new(source_stream);
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
      (source, config)
    }
  };

  Ok((temp_path, source, config))
}

async fn create_service<'a>(
  state: &'a ServerState,
  mode: UploadMode,
  name: String,
  config: Config,
  source: Source,
  source_kind: SourceKind,
  temp_path: &Path,
) -> Result<UploadResponse<'a>> {
  let (new_service, replaced_service, errors) = match mode {
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
  let guard = new_service.upgrade();

  let service_path = state.abel_path.join("services").join(guard.name());
  if service_path.exists() {
    fs::remove_dir_all(&service_path).await?;
  }
  fs::create_dir(&service_path).await?;

  let metadata = Metadata {
    uuid: guard.uuid(),
    started: true,
  };
  metadata.write(&service_path.join("metadata.json")).await?;

  match source_kind {
    SourceKind::Single => fs::rename(temp_path, service_path.join("source.lua")).await?,
    SourceKind::Multi => fs::hard_link(temp_path, service_path.join("source.asar")).await?,
  }

  Ok(UploadResponse {
    new_service,
    replaced_service,
    errors,
  })
}

pub fn log_result(
  UploadResponse {
    new_service,
    replaced_service,
    errors,
  }: &UploadResponse,
) {
  let service = new_service.upgrade();
  if let Some(replaced) = replaced_service {
    info!(
      "Updated service '{}' {}",
      service.name(),
      format!("({} -> {})", replaced.uuid(), service.uuid()).dimmed(),
    );
  } else {
    info!(
      "Created service '{}' {}",
      service.name(),
      format!("({})", service.uuid()).dimmed(),
    );
  }
  if !errors.is_empty() {
    warn!("errors: {errors:?}");
  }
}

async fn response(resp: UploadResponse<'_>) -> Result<Response<Body>> {
  log_result(&resp);
  let UploadResponse {
    new_service,
    replaced_service,
    errors,
  } = resp;

  let guard = new_service.upgrade();
  let body = HttpUploadResponse {
    new_service: ServiceWithStatus::from_guard(&guard),
    replaced_service: replaced_service.as_ref().map(|x| Cow::Borrowed(x.info())),
    errors: errors.into(),
  };
  json_response(StatusCode::OK, body)
}
