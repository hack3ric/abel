use crate::metadata::Metadata;
use crate::util::{asyncify, json_response};
use crate::{MainState, Result};
use futures::TryStreamExt;
use hive_asar::Archive;
use hive_core::ErrorKind::ServiceExists;
use hive_core::{Config, RunningService, ServiceImpl, Source};
use hyper::{Body, HeaderMap, Request, Response, StatusCode};
use log::info;
use multer::{Constraints, Field, Multipart, SizeLimit};
use serde_json::json;
use std::path::Path;
use tempfile::{tempdir, tempfile};
use tokio::fs::{self, File};
use tokio::io::{self, AsyncWrite};
use tokio_util::io::StreamReader;

pub(crate) async fn upload(
  state: &MainState,
  name: Option<String>,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let (parts, body) = req.into_parts();
  let mut multipart = parse_multipart(&parts.headers, body)?;

  let source_field = multipart.next_field().await?.ok_or((
    400,
    "no source uploaded",
    "specify either `single` or `multi` field in multipart",
  ))?;
  let tmp = asyncify(tempdir).await?;

  let config = match source_field.name() {
    Some("single") => read_single(tmp.path(), multipart, source_field).await?,
    Some("multi") => read_multi(tmp.path(), source_field).await?,
    _ => {
      return Err(From::from((
        400,
        "unknown field name",
        "first field is neither named `single` nor `multi`",
      )))
    }
  };

  if config.is_none() {
    fs::write(tmp.path().join("hive.json"), "{}").await?;
  }

  let (name, config) = get_name_config(state, name, config).await?;
  let (service, replaced) = create_service(state, name, config, tmp.into_path()).await?;
  response(service, replaced).await
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

async fn read_single<'a>(
  tmp: &Path,
  mut multipart: Multipart<'static>,
  source_field: Field<'static>,
) -> Result<Option<Config>> {
  let mut main = File::create(tmp.join("main.lua")).await?;
  save_field(source_field, &mut main).await?;

  if let Some(config_field) = multipart.next_field().await? {
    // FIXME: use `Option::contains` when it stablizes
    if config_field.name().map(|x| x != "config").unwrap_or(true) {
      return Err(From::from((
        400,
        "unknown field name",
        "second field is not named `config`",
      )));
    }
    let bytes = config_field.bytes().await?;
    let config: Config = serde_json::from_slice(&bytes)?;
    fs::write(tmp.join("hive.json"), &bytes).await?;
    Ok(Some(config))
  } else {
    Ok(None)
  }
}

async fn read_multi<'a>(tmp: &Path, source_field: Field<'static>) -> Result<Option<Config>> {
  let mut tmpfile = File::from_std(asyncify(tempfile).await?);
  save_field(source_field, &mut tmpfile).await?;
  let mut archive = Archive::new(tmpfile).await?;
  archive.extract(tmp).await?;

  if let Ok(bytes) = fs::read(tmp.join("hive.json")).await {
    Ok(Some(serde_json::from_slice(&bytes)?))
  } else {
    Ok(None)
  }
}

async fn save_field(field: Field<'_>, dest: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
  let mut stream =
    StreamReader::new(field.map_err(|error| io::Error::new(io::ErrorKind::Other, error)));
  io::copy(&mut stream, dest).await?;
  Ok(())
}

async fn get_name_config(
  state: &MainState,
  name: Option<String>,
  config: Option<Config>,
) -> Result<(String, Config)> {
  let name_provided = name.is_some();

  let (name, config) = if let Some(config) = config {
    let name = name
      .or_else(|| {
        config.pkg_name.as_ref().map(|x| {
          let x = x.rsplit_once('.').map(|x| x.0).unwrap_or(x);
          slug::slugify(x)
        })
      })
      .ok_or((
        400,
        "no service name provided",
        "neither service name in path nor config's `pkg_name` field is specified",
      ))?;
    (name, config)
  } else {
    let name = name.ok_or((
      400,
      "no service name provided",
      "missing config; service name not specified in path",
    ))?;
    (name, Default::default())
  };

  if !name_provided && state.hive.get_running_service(&name).is_ok() {
    return Err(ServiceExists { name: name.into() }.into());
  }

  Ok((name, config))
}

async fn create_service(
  state: &MainState,
  name: String,
  config: Config,
  source_path: impl AsRef<Path>,
) -> Result<(RunningService, Option<ServiceImpl>)> {
  let source = Source::new(source_path.as_ref()).await?;
  let (service, replaced) = (state.hive)
    .create_service(name, source.clone(), config)
    .await?;

  let guard = service.upgrade();
  let name = guard.name();

  let service_path = state.config_path.join("services").join(&name);
  if service_path.exists() {
    fs::remove_dir_all(&service_path).await?;
  }
  fs::create_dir(&service_path).await?;
  source.rename_base(service_path.join("src")).await?;

  let metadata = Metadata {
    uuid: guard.uuid(),
    started: true,
  };
  fs::write(
    service_path.join("metadata.json"),
    serde_json::to_string(&metadata)?,
  )
  .await?;

  Ok((service, replaced))
}

async fn response(
  service: RunningService,
  replaced: Option<ServiceImpl>,
) -> Result<Response<Body>> {
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
