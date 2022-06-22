use crate::Result;
use async_trait::async_trait;
use hive_core::path::normalize_path_str;
use hive_core::source::SourceVfs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{canonicalize, rename, File};
use tokio::io;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct DirSource(Arc<RwLock<PathBuf>>);

impl DirSource {
  pub async fn new(base: impl AsRef<Path>) -> Result<Self> {
    let base = canonicalize(base).await?;
    Ok(Self(Arc::new(RwLock::new(base))))
  }

  pub async fn rename_base(&self, new_path: PathBuf) -> Result<()> {
    let mut base = self.0.write().await;
    rename(&*base, &new_path).await?;
    *base = new_path;
    Ok(())
  }
}

#[async_trait]
impl SourceVfs for DirSource {
  type File = File;

  async fn get(&self, path: &str) -> io::Result<File> {
    let path = normalize_path_str(path);
    let file = File::open(self.0.read().await.join(path)).await?;
    Ok(file)
  }

  async fn exists(&self, path: &str) -> io::Result<bool> {
    Ok(self.0.read().await.join(normalize_path_str(path)).exists())
  }
}
