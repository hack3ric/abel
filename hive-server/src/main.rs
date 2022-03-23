mod error;
mod handle;
mod metadata;
#[macro_use]
mod util;

use crate::handle::handle;
use crate::metadata::Metadata;
use error::Error;
use hive_core::{Hive, HiveOptions, Source};
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::io::AsyncReadExt;
use tokio::{fs, io};
use uuid::Uuid;

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(StructOpt)]
struct Opt {
  #[structopt(short = "l", long = "listen", default_value = "127.0.0.1:3000")]
  addr: SocketAddr,

  #[structopt(long)]
  pool_size: Option<usize>,
}

pub(crate) struct MainState {
  hive: Hive,
  config_path: PathBuf,
  auth_token: Option<Uuid>,
}

static HALF_NUM_CPUS: Lazy<usize> = Lazy::new(|| 1.max(num_cpus::get() / 2));

async fn run() -> anyhow::Result<()> {
  if option_env!("RUST_LOG").is_none() {
    std::env::set_var("RUST_LOG", "INFO");
  }
  pretty_env_logger::init();
  let opt = Opt::from_args();

  let mut config_path = home::home_dir().expect("no home directory found");
  config_path.push(".hive");
  let local_storage_path = async {
    if !config_path.exists() {
      fs::create_dir(&config_path).await?;
    }
    let services_path = config_path.join("services");
    if !services_path.exists() {
      fs::create_dir(&services_path).await?;
    }
    let local_storage_path = config_path.join("storage");
    if !local_storage_path.exists() {
      fs::create_dir(&local_storage_path).await?;
    }
    Ok::<_, io::Error>(local_storage_path)
  }
  .await
  .expect("failed to create Hive config directory");

  let state = Arc::new(MainState {
    hive: Hive::new(HiveOptions {
      sandbox_pool_size: opt.pool_size.unwrap_or(*HALF_NUM_CPUS),
      local_storage_path,
    })?,
    config_path: config_path.clone(),
    // auth_token: Some(Uuid::new_v4()),
    auth_token: None,
  });

  if let Some(auth_token) = &state.auth_token {
    info!("Authentication token: {auth_token}");
  } else {
    warn!("No authentication token set. Don't do this in production environment!");
  }

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

  let state2 = state.clone();
  let make_svc = make_service_fn(move |_conn| {
    let state = state2.clone();
    async move { Ok::<_, Infallible>(service_fn(move |req| handle(state.clone(), req))) }
  });

  let server = Server::bind(&opt.addr)
    .serve(make_svc)
    .with_graceful_shutdown(shutdown_signal());

  info!("Hive is listening to {}", opt.addr);

  if let Err(error) = server.await {
    error!("fatal server error: {}", error);
  }

  state.hive.stop_all_services().await;

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
