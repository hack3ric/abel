mod deploy;
mod dev;
mod resolve;
mod server;
mod source;

use crate::dev::save_services_from_paths;
use clap::{Parser, Subcommand};
use deploy::deploy;
use dev::init_watcher;
use futures::Future;
use hyper::Uri;
use log::{info, warn};
use owo_colors::OwoColorize;
use resolve::resolve_dep;
use server::config::{Config, ConfigArgs, ServerArgs, HALF_NUM_CPUS};
use server::upload::UploadMode;
use server::{init_logger, init_state, init_state_with_stored_config, load_saved_services};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::io;
use uuid::Uuid;

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
  Deploy {
    #[clap(short, long)]
    server: Option<Uri>,
    #[clap(short, long)]
    auth_token: Option<Uuid>,
    path: PathBuf,
    #[clap(short, long, value_enum, default_value_t)]
    mode: UploadMode,
  },
  Resolve {
    path: PathBuf,
  },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
  Single,
  Multi,
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

        load_saved_services(&state, &abel_path.join("services")).await?;
        server::run(config, state).await
      })
    }
    Command::Dev { config, services } => {
      init_logger();
      info!("Starting abel-server v{ver} (dev mode)");

      let abel_path = tempdir()?;
      info!(
        "Abel working path: {}",
        abel_path.path().display().underline()
      );
      let server_args = ServerArgs {
        config,
        abel_path: abel_path.path().into(),
      };
      let default_config = Config {
        auth_token: None,
        ..Default::default()
      };
      let services = services
        .into_iter()
        .map(std::fs::canonicalize)
        .collect::<io::Result<Arc<[_]>>>()?;

      block_on(async {
        let (_, config, state) = init_state(server_args, default_config).await?;

        let services_path = abel_path.path().join("services");
        let kinds_and_names = save_services_from_paths(&services, &services_path).await?;

        load_saved_services(&state, &services_path).await?;
        let server_handle = tokio::spawn(server::run(config, state.clone()));
        let _watcher = init_watcher(state, kinds_and_names, services)?;
        server_handle.await?
      })
    }
    Command::Deploy {
      server,
      auth_token,
      path,
      mode,
    } => {
      if let Err(error) = block_on(deploy(server, auth_token, path, mode)) {
        println!("{} {error:?}", "error:".red().bold());
        std::process::exit(1);
      }
      Ok(())
    }
    Command::Resolve { path } => {
      block_on(resolve_dep(path))?;
      Ok(())
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
