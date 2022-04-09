mod error;
mod handle;
mod metadata;
#[macro_use]
mod util;
mod config;

use clap::Parser;
use config::{Args, HALF_NUM_CPUS};
use error::Error;
use handle::handle;
use hive_core::{Hive, HiveOptions, Source};
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use log::{error, info, warn};
use metadata::Metadata;
use std::convert::Infallible;
use std::path::{PathBuf, Path};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::{fs, io};
use uuid::Uuid;
use crate::config::Config;

type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) struct MainState {
  hive: Hive,
  hive_path: PathBuf,
  auth_token: Option<Uuid>,
}

async fn run() -> anyhow::Result<()> {
  if option_env!("RUST_LOG").is_none() {
    std::env::set_var("RUST_LOG", "INFO");
  }
  pretty_env_logger::init();
  let args = Args::parse();

  let (hive_path, local_storage_path) = init_paths().await;
  
  let config_path = hive_path.join("config.json");
  let config = Config::get(config_path, args.config).await?;

  let state = Arc::new(MainState {
    hive: Hive::new(HiveOptions {
      sandbox_pool_size: config.pool_size(),
      local_storage_path,
    })?,
    hive_path: hive_path.clone(),
    auth_token: Some(config.auth_token),
  });

  if let Some(auth_token) = &state.auth_token {
    info!("Authentication token: {auth_token}");
  } else {
    warn!("No authentication token set. Don't do this in production environment!");
  }

  load_saved_services(&state, hive_path).await?;

  let state2 = state.clone();
  let make_svc = make_service_fn(move |_conn| {
    let state = state2.clone();
    async move { Ok::<_, Infallible>(service_fn(move |req| handle(state.clone(), req))) }
  });

  let server = Server::bind(&config.listen)
    .serve(make_svc)
    .with_graceful_shutdown(shutdown_signal());

  info!("Hive is listening to {}", config.listen);

  if let Err(error) = server.await {
    error!("fatal server error: {}", error);
  }

  state.hive.stop_all_services().await;

  Ok(())
}

async fn init_paths() -> (PathBuf, PathBuf) {
  async fn create_dir_path<T: AsRef<Path>>(path: T) -> io::Result<T> {
    if !(&path).as_ref().exists() {
      fs::create_dir(&path).await?;
    }
    Ok(path)
  }

  let mut hive_path = home::home_dir().expect("no home directory found");
  hive_path.push(".hive");

  let local_storage_path = async {
    create_dir_path(&hive_path).await?;
    create_dir_path(hive_path.join("services")).await?;
    create_dir_path(hive_path.join("storage")).await
  }
  .await
  .expect("failed to create Hive config directory");

  (hive_path, local_storage_path)
}

async fn load_saved_services(state: &MainState, config_path: PathBuf) -> Result<()> {
  let mut services = fs::read_dir(config_path.join("services")).await?;

  while let Some(service_folder) = services.next_entry().await? {
    if service_folder.file_type().await?.is_dir() {
      let name = service_folder.file_name().to_string_lossy().into_owned();
      let result = async {
        let metadata_bytes = fs::read(service_folder.path().join("metadata.json")).await?;
        let metadata: Metadata = serde_json::from_slice(&metadata_bytes)?;

        let source = Source::new(service_folder.path().join("src")).await?;
        let mut config = source.get("hive.json").await?;
        let mut bytes = Vec::with_capacity(config.metadata().await?.len() as _);
        config.read_to_end(&mut bytes).await?;
        let config = serde_json::from_slice(&bytes)?;

        if metadata.started {
          // TODO: if failed to create service, load it instead
          // Don't forget to change `metadata.json` too
          let (service, _) = (state.hive)
            .create_service(name.clone(), Some(metadata.uuid), source, config)
            .await?;
          let service = service.upgrade();
          info!("Loaded service '{}' ({})", service.name(), service.uuid())
        } else {
          let service = (state.hive)
            .load_service(name.clone(), metadata.uuid, source, config)
            .await?;
          info!("Loaded service '{}' ({})", service.name(), service.uuid())
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

fn main() -> anyhow::Result<()> {
  tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(*HALF_NUM_CPUS)
    .build()
    .unwrap()
    .block_on(run())
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
