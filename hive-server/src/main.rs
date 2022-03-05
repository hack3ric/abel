mod error;
mod handle;
#[macro_use]
mod util;

use crate::handle::handle;
use hive_core::{Hive, HiveOptions};
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use log::{error, info};
use once_cell::sync::Lazy;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::{fs, io};

type Result<T, E = error::Error> = std::result::Result<T, E>;

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
      local_storage_path
    })?,
    config_path,
  });

  let make_svc = make_service_fn(move |_conn| {
    let state = state.clone();
    async move { Ok::<_, Infallible>(service_fn(move |req| handle(state.clone(), req))) }
  });

  let server = Server::bind(&opt.addr).serve(make_svc);

  info!("Hive is listening to {}", opt.addr);
  if let Err(error) = server.await {
    error!("fatal server error: {}", error);
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
