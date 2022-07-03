mod config;
mod error;
mod handle;
mod metadata;
mod source;
mod util;

use crate::config::Config;
use abel_core::service::Service;
use abel_core::source::{Source, SourceVfs};
use abel_core::{Abel, AbelOptions};
use clap::Parser;
use config::{Args, HALF_NUM_CPUS};
use error::Error;
use handle::handle;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use log::{error, info, warn};
use metadata::Metadata;
use owo_colors::OwoColorize;
use source::DirSource;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::{fs, io};
use uuid::Uuid;

type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) struct MainState {
  abel: Abel,
  abel_path: PathBuf,
  auth_token: Option<Uuid>,
}

async fn run() -> anyhow::Result<()> {
  if option_env!("RUST_LOG").is_none() {
    std::env::set_var("RUST_LOG", "INFO");
  }
  pretty_env_logger::init();
  info!("Starting abel-server v{}", env!("CARGO_PKG_VERSION"));

  let Args { config, abel_path } = Args::parse();

  info!("Abel working path: {}", abel_path.display().underline());
  let local_storage_path = init_paths(&abel_path).await;

  let config_path = abel_path.join("config.json");
  let config = Config::get(config_path, config).await?;

  let state = Arc::new(MainState {
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
    if !(&path).as_ref().exists() {
      fs::create_dir(&path).await?;
    }
    Ok(())
  }

  let local_storage_path = async {
    create_dir_path(abel_path).await?;
    create_dir_path(abel_path.join("services")).await?;
    create_dir_path(abel_path.join("tmp")).await?;

    let local_storage_path = abel_path.join("storage");
    create_dir_path(&local_storage_path).await?;
    io::Result::Ok(local_storage_path)
  }
  .await
  .expect("failed to create Abel config directory");

  local_storage_path
}

async fn load_saved_services(state: &MainState, config_path: PathBuf) -> Result<()> {
  let mut services = fs::read_dir(config_path.join("services")).await?;

  while let Some(service_folder) = services.next_entry().await? {
    if service_folder.file_type().await?.is_dir() {
      let name = service_folder.file_name().to_string_lossy().into_owned();
      let result = async {
        let metadata_bytes = fs::read(service_folder.path().join("metadata.json")).await?;
        let metadata: Metadata = serde_json::from_slice(&metadata_bytes)?;

        let source = DirSource::new(service_folder.path().join("src")).await?;
        let mut config = source.get("abel.json").await?;
        let mut bytes = Vec::with_capacity(config.metadata().await?.len() as _);
        config.read_to_end(&mut bytes).await?;
        let config = serde_json::from_slice(&bytes)?;

        let (service, error_payload) = if metadata.started {
          let (service, _, error_payload) = (state.abel)
            .cold_update_or_create_service(
              name.clone(),
              Some(metadata.uuid),
              Source::new(source),
              config,
            )
            .await?;
          (service, error_payload)
        } else {
          let (service, error_payload) = (state.abel)
            .preload_service(name.clone(), metadata.uuid, Source::new(source), config)
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

        Ok::<_, crate::Error>(())
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

fn main() -> anyhow::Result<()> {
  tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(*HALF_NUM_CPUS)
    .build()
    .unwrap()
    .block_on(run())
}
