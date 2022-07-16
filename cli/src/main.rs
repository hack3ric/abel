mod server;

use clap::{Parser, Subcommand};
use futures::Future;
use server::{ConfigArgs, ServerArgs, HALF_NUM_CPUS, Config};
use std::path::PathBuf;
use tempfile::tempdir;

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
  #[clap(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  /// Run Abel server.
  Server {
    #[clap(flatten)]
    args: ServerArgs,
  },
  Dev {
    #[clap(flatten)]
    config: ConfigArgs,
    services: Vec<PathBuf>,
  },
}

fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  match args.command {
    Command::Server { args } => block_on(server::run_with_stored_config(args)),
    Command::Dev { config, services } => {
      let abel_path = tempdir()?;
      let server_args = ServerArgs {
        config,
        abel_path: abel_path.path().into(),
      };
      // TODO: "upload" services
      block_on(server::run(
        server_args,
        Config {
          auth_token: None,
          ..Default::default()
        },
      ))
    }
  }
}

fn block_on<F: Future>(f: F) -> F::Output {
  tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(*HALF_NUM_CPUS)
    .build()
    .unwrap()
    .block_on(f)
}
