use crate::server::metadata::Metadata;
use crate::server::upload::{log_result, upload_local, UploadMode};
use crate::server::ServerState;
use crate::SourceKind;
use anyhow::anyhow;
use futures::TryFutureExt;
use hive_asar::pack_dir_into_stream;
use log::{error, warn};
use notify::RecursiveMode::Recursive;
use notify::{Event, RecommendedWatcher, Watcher};
use slug::slugify;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt};
use tokio::runtime::Handle;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

pub async fn save_services_from_paths(
  services: &[PathBuf],
  services_path: &Path,
) -> anyhow::Result<Vec<(SourceKind, String)>> {
  let mut kinds_and_names = Vec::with_capacity(services.len());
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
    kinds_and_names.push((kind, name));

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

  Ok(kinds_and_names)
}

pub fn init_watcher(
  state: Arc<ServerState>,
  kinds_and_names: Vec<(SourceKind, String)>,
  services: Arc<[PathBuf]>,
) -> anyhow::Result<RecommendedWatcher> {
  let rt = Handle::current();
  let mut time = Instant::now();

  let mut watcher = notify::recommended_watcher({
    let services = services.clone();
    move |result: Result<Event, notify::Error>| {
      let now = Instant::now();
      let dur = now.duration_since(time);
      match result {
        Ok(event) if dur > Duration::from_millis(100) => {
          time = now;
          let mut event_paths_iter = event.paths.into_iter();
          'services: for ((kind, name), path) in kinds_and_names.iter().zip(&*services) {
            if event_paths_iter.len() == 0 {
              break;
            }
            for event_path in &mut event_paths_iter {
              if &event_path == path || *kind == SourceKind::Multi && event_path.starts_with(path) {
                let result = rt.block_on(async {
                  const MODE: UploadMode = UploadMode::Hot; // FIXME: is hot update okay?
                  let resp = match kind {
                    SourceKind::Single => {
                      let stream = ReaderStream::new(File::open(&path).await?);
                      upload_local(&state, name.clone(), MODE, *kind, stream).await?
                    }
                    SourceKind::Multi => {
                      let stream = pack_dir_into_stream(&path).await?;
                      upload_local(&state, name.clone(), MODE, *kind, stream).await?
                    }
                  };
                  log_result(&resp);
                  anyhow::Ok(())
                });

                if let Err(error) = result {
                  warn!("Error updating service '{name}': {error}");
                  warn!("maybe check '{}'?", path.display());
                }

                continue 'services;
              }
            }
          }
        }
        Ok(_) => {}
        Err(error) => error!("failed to watch files: {error}"),
      }
    }
  })?;

  for path in &*services {
    watcher.watch(path, Recursive)?;
  }

  Ok(watcher)
}
