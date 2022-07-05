use crate::error::{Error, ErrorKind};
use crate::metadata::Metadata;
use crate::source::{AsarSource, SingleSource};
use crate::util::json_response;
use crate::{MainState, Result};
use abel_core::service::{ErrorPayload, Service};
use abel_core::source::Source;
use abel_core::ErrorKind::ServiceExists;
use abel_core::{Config, ServiceImpl};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum UploadMode {
  #[serde(rename = "create")]
  Create,
  #[serde(rename = "hot")]
  Hot,
  #[serde(rename = "cold")]
  Cold,
  #[serde(rename = "load")]
  Load,
}

impl Default for UploadMode {
  fn default() -> Self {
    Self::Hot
  }
}

#[derive(Deserialize)]
struct UploadQuery {
  #[serde(default)]
  mode: UploadMode,
}

pub(crate) async fn upload(
  state: &MainState,
  name: String,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let (parts, body) = req.into_parts();
  let mut multipart = parse_multipart(&parts.headers, body)?;

  let UploadQuery { mode } = serde_qs::from_str(parts.uri.query().unwrap_or(""))?;

  // TODO: check name in `state.abel`

  let source_field = multipart.next_field().await?.ok_or((
    "no source uploaded",
    "specify either `single` or `multi` field in multipart",
  ))?;

  // create service dir, if the following errors then remove the folder
  let service_path = state.abel_path.join("services").join(&name);
  if service_path.exists() {
    fs::remove_dir_all(&service_path).await?;
  }
  fs::create_dir(&service_path).await?;

  let result = async {
    let (source, config): (Source, Config) = match source_field.name() {
      Some("single") => {
        let code = source_field.bytes().await?;
        fs::write(service_path.join("source.lua"), &code).await?;

        let source = Source::new(SingleSource::new(code));
        (source, Default::default())
      }
      Some("multi") => {
        let mut reader =
          StreamReader::new(source_field.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        let path = service_path.join("source.asar");
        let mut writer = File::create(&path).await?;
        io::copy(&mut reader, &mut writer).await?;

        let mut archive = Archive::new_from_file(path).await?;

        let mut config_file = archive.get("abel.json").await?;
        let mut config_bytes = vec![0; config_file.metadata().size as _];
        config_file.read_to_end(&mut config_bytes).await?;
        let config = serde_json::from_slice(&config_bytes)?;

        let source = Source::new(AsarSource(archive));
        (source, config)
      }
      _ => {
        return Err(From::from((
          "unknown field name",
          "first field is neither named `single` nor `multi`",
        )))
      }
    };

    create_service(state, mode, name, config, source, &service_path).await
  };

  match result.await {
    Ok((service, replaced, error_payload)) => response(service, replaced, error_payload).await,
    Err(error) => {
      fs::remove_dir_all(service_path).await?;
      Err(error)
    }
  }
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
  state: &'a MainState,
  mode: UploadMode,
  name: String,
  config: Config,
  source: Source,
  service_path: &Path,
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

  let metadata = Metadata {
    uuid: guard.uuid(),
    started: true,
  };
  fs::write(
    service_path.join("metadata.json"),
    serde_json::to_string(&metadata)?,
  )
  .await?;

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
