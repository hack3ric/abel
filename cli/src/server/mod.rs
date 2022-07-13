mod config;
mod error;
mod handle;
mod metadata;
mod source;

use self::metadata::Metadata;
use self::source::{AsarSource, SingleSource};
use abel_core::service::Service;
use abel_core::{Abel, AbelOptions};
use abel_rt::Source;
use anyhow::bail;
use config::Config;
use error::Error;
use handle::handle;
use hive_asar::Archive;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use log::{error, info, warn};
use owo_colors::OwoColorize;
use serde::Serialize;
use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

pub use config::{ServerArgs, HALF_NUM_CPUS};

type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) struct ServerState {
  abel: Abel,
  abel_path: PathBuf,
  auth_token: Option<Uuid>,
}

pub async fn run(args: ServerArgs) -> anyhow::Result<()> {
  if option_env!("RUST_LOG").is_none() {
    std::env::set_var("RUST_LOG", "INFO");
  }
  pretty_env_logger::init();
  info!("Starting abel-server v{}", env!("CARGO_PKG_VERSION"));

  let ServerArgs { config, abel_path } = args;

  info!("Abel working path: {}", abel_path.display().underline());
  let local_storage_path = init_paths(&abel_path).await;

  let config_path = abel_path.join("config.json");
  let config = Config::get(config_path, config).await?;

  let state = Arc::new(ServerState {
    abel: Abel::new(AbelOptions {
      runtime_pool_size: config.pool_size(),
      local_storage_path,
    })?,
    abel_path: abel_path.clone(),
    auth_token: Some(config.auth_token),
  });

  if let Some(auth_token) = &state.auth_token {
    info!("Authentication token: {auth_token}");
  } else {
    warn!("No authentication token set. Don't do this in production environment!");
  }

  load_saved_services(&state, abel_path).await?;

  let state2 = state.clone();
  let make_svc = make_service_fn(move |_conn| {
    let state = state2.clone();
    async move { Ok::<_, Infallible>(service_fn(move |req| handle(state.clone(), req))) }
  });

  let server = Server::bind(&config.listen)
    .serve(make_svc)
    .with_graceful_shutdown(shutdown_signal());

  info!("Abel is listening to {}", config.listen.underline());

  if let Err(error) = server.await {
    error!("fatal server error: {}", error);
  }

  state.abel.stop_all_services().await;

  Ok(())
}

async fn init_paths(abel_path: &Path) -> PathBuf {
  async fn create_dir_path(path: impl AsRef<Path>) -> io::Result<()> {
    if !path.as_ref().exists() {
      fs::create_dir(&path).await?;
    }
    Ok(())
  }

  let local_storage_path = async {
    create_dir_path(abel_path).await?;
    create_dir_path(abel_path.join("services")).await?;

    // Creates a fresh temporary folder
    let temp_dir = abel_path.join("tmp");
    if temp_dir.exists() {
      fs::remove_dir_all(&temp_dir).await?;
    }
    fs::create_dir(temp_dir).await?;

    let local_storage_path = abel_path.join("storage");
    create_dir_path(&local_storage_path).await?;
    io::Result::Ok(local_storage_path)
  }
  .await
  .expect("failed to create Abel config directory");

  local_storage_path
}

async fn load_saved_services(state: &ServerState, config_path: PathBuf) -> anyhow::Result<()> {
  let mut services = fs::read_dir(config_path.join("services")).await?;

  while let Some(service_folder) = services.next_entry().await? {
    if service_folder.file_type().await?.is_dir() {
      let name = service_folder.file_name().to_string_lossy().into_owned();
      let result = async {
        let metadata_bytes = fs::read(service_folder.path().join("metadata.json")).await?;
        let metadata: Metadata = serde_json::from_slice(&metadata_bytes)?;

        let asar_path = service_folder.path().join("source.asar");
        let asar_exists = asar_path.exists();
        let lua_path = service_folder.path().join("source.lua");
        let lua_exists = lua_path.exists();
        let (source, config) = if asar_exists && lua_exists {
          bail!("both source.asar and source.lua found");
        } else if asar_exists {
          let mut archive = Archive::new_from_file(asar_path).await?;

          let config = if let Ok(mut config_file) = archive.get("abel.json").await {
            let mut config_bytes = vec![0; config_file.metadata().size as _];
            config_file.read_to_end(&mut config_bytes).await?;
            serde_json::from_slice(&config_bytes)?
          } else {
            Default::default()
          };

          let source = Source::new(AsarSource(archive));
          (source, config)
        } else if lua_exists {
          let code = fs::read(lua_path).await?;
          let source = Source::new(SingleSource::new(code));
          (source, Default::default())
        } else {
          bail!("neither source.asar nor source.lua found")
        };

        let (service, error_payload) = if metadata.started {
          let (service, _, error_payload) = (state.abel)
            .cold_update_or_create_service(name.clone(), Some(metadata.uuid), source, config)
            .await?;
          (service, error_payload)
        } else {
          let (service, error_payload) = (state.abel)
            .preload_service(name.clone(), metadata.uuid, source, config)
            .await?;
          (Service::Stopped(service), error_payload)
        };

        let service = service.upgrade();
        if !error_payload.is_empty() {
          warn!(
            "Loaded service '{}' with error {}",
            service.name(),
            format!("({})", service.uuid()).dimmed(),
          );
          warn!("error payload: {error_payload:?}");
        } else {
          info!(
            "Loaded service '{}' {}",
            service.name(),
            format!("({})", service.uuid()).dimmed()
          );
        }

        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        warn!(
          "Error preloading service '{name}': {error}; maybe check {:?}?",
          service_folder.path()
        )
      }
    }
  }
  Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() {
  use tokio::select;
  use tokio::signal::unix::{signal, SignalKind};

  let mut sigint = signal(SignalKind::interrupt()).unwrap();
  let mut sigterm = signal(SignalKind::terminate()).unwrap();

  let signal = select! {
    _ = sigint.recv() => "SIGINT",
    _ = sigterm.recv() => "SIGTERM",
  };

  info!("{signal} received; gracefully shutting down");
}

#[cfg(windows)]
async fn shutdown_signal() {
  tokio::signal::ctrl_c().await.unwrap();
  info!("gracefully shutting down");
}

pub fn json_response(status: StatusCode, body: impl Serialize) -> Result<Response<Body>> {
  Ok(json_response_raw(status, body))
}

pub fn json_response_raw(status: StatusCode, body: impl Serialize) -> Response<Body> {
  Response::builder()
    .status(status)
    .header("content-type", "application/json")
    .body(serde_json::to_string(&body).unwrap().into())
    .unwrap()
}

pub(crate) fn authenticate(state: &ServerState, req: &Request<Body>) -> bool {
  let result = if let Some(uuid) = state.auth_token {
    (req.headers())
      .get("authorization")
      .map(|x| x == &format!("Abel {uuid}"))
      .unwrap_or(false)
  } else {
    true
  };
  result
}
