mod server;

use crate::server::{init_state, init_state_with_stored_config, load_saved_services, Metadata};
use anyhow::anyhow;
use clap::{Parser, Subcommand};
use futures::{Future, TryFutureExt};
use log::{info, warn};
use owo_colors::OwoColorize;
use server::{init_logger, Config, ConfigArgs, ServerArgs, HALF_NUM_CPUS};
use slug::slugify;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
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

        let services_path = abel_path.path().join("services");
        save_services_from_paths(services, &services_path).await?;

        // TODO: watch changes

        load_saved_services(&state, &services_path).await?;
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

async fn save_services_from_paths(
  services: Vec<PathBuf>,
  services_path: &Path,
) -> anyhow::Result<()> {
  for path in services {
    let path = fs::canonicalize(path).await?;
    let fs_metadata = fs::metadata(&path).await?;
    let name = path.file_name();
    let (kind, name) = if fs_metadata.is_file() {
      let name = name
        .ok_or_else(|| anyhow!("file name is missing"))?
        .to_str()
        .map(|x| slugify(x.rsplit_once('.').unwrap_or((x, "")).0))
        .ok_or_else(|| anyhow!("file name is not UTF-8"))?;
      (SourceKind::Single, name)
    } else {
      let config_name = File::open(path.join("abel.json"))
        .and_then(|mut f| async move {
          let mut config_bytes = Vec::new();
          f.read_to_end(&mut config_bytes).await?;
          let result: abel_core::Config = serde_json::from_slice(&config_bytes)?;
          io::Result::Ok(result.pkg_name)
        })
        .await
        .ok()
        .flatten();
      let name = config_name
        .map(slugify)
        .or_else(|| {
          name
            .and_then(|x| x.to_str())
            .map(|x| slugify(x.rsplit_once('.').unwrap_or((x, "")).0))
        })
        .ok_or_else(|| anyhow!("no appropriate name found"))?;
      (SourceKind::Multi, name)
    };

    let service_path = services_path.join(&name);
    if service_path.exists() {
      warn!("service '{name}' already exists; skipping");
      continue;
    }
    fs::create_dir(&service_path).await?;
    Metadata {
      uuid: Uuid::new_v4(),
      started: true,
    }
    .write(&service_path.join("metadata.json"))
    .await?;

    match kind {
      SourceKind::Single => {
        let mut file = File::open(&path).await?;
        let mut dest = File::create(service_path.join("source.lua")).await?;
        io::copy(&mut file, &mut dest).await?;
      }
      SourceKind::Multi => {
        let mut dest = File::create(service_path.join("source.asar")).await?;
        hive_asar::pack_dir(path, &mut dest).await?;
      }
    }
  }

  Ok(())
}
