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
use structopt::StructOpt;

type Result<T, E = error::Error> = std::result::Result<T, E>;

#[derive(StructOpt)]
struct Opt {
  #[structopt(short = "l", long = "listen", default_value = "127.0.0.1:3000")]
  addr: SocketAddr,

  #[structopt(long)]
  pool_size: Option<usize>,
}

static HALF_NUM_CPUS: Lazy<usize> = Lazy::new(|| 1.max(num_cpus::get() / 2));

async fn _main() -> anyhow::Result<()> {
  if option_env!("RUST_LOG").is_none() {
    std::env::set_var("RUST_LOG", "INFO");
  }
  pretty_env_logger::init();
  let opt = Opt::from_args();

  let hive = Hive::new(HiveOptions {
    sandbox_pool_size: opt.pool_size.unwrap_or(*HALF_NUM_CPUS),
  })?;

  let make_svc = make_service_fn(move |_conn| {
    let hive = hive.clone();
    async move { Ok::<_, Infallible>(service_fn(move |req| handle(hive.clone(), req))) }
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
    .block_on(_main())
}
