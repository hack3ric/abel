mod server;

use crate::server::{init_state, init_state_with_stored_config, load_saved_services};
use clap::{Parser, Subcommand};
use futures::Future;
use log::{info, warn};
use owo_colors::OwoColorize;
use server::{init_logger, Config, ConfigArgs, ServerArgs, HALF_NUM_CPUS};
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
  let ver = env!("CARGO_PKG_VERSION");

  match args.command {
    Command::Server { args } => {
      init_logger();
      info!("Starting abel-server v{ver}");
      block_on(async {
        let (abel_path, config, state) = init_state_with_stored_config(args).await?;
        info!("Abel working path: {}", abel_path.display().underline());

        if let Some(auth_token) = &state.auth_token {
          info!("Authentication token: {auth_token}");
        } else {
          warn!("No authentication token set. Don't do this in production environment!");
        }

        load_saved_services(&state, abel_path).await?;
        server::run(config, state).await
      })
    }
    Command::Dev { config, services } => {
      init_logger();
      info!("Starting abel-server v{ver} (dev mode)");

      let abel_path = tempdir()?;
      let server_args = ServerArgs {
        config,
        abel_path: abel_path.path().into(),
      };
      let default_config = Config {
        auth_token: None,
        ..Default::default()
      };

      block_on(async {
        let (_, config, state) = init_state(server_args, default_config).await?;
        // TODO: "upload" services
        server::run(config, state).await
      })
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
