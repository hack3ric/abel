use super::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::{fs, io};
use uuid::Uuid;

/// Extra information of a loaded service.
#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
  pub uuid: Uuid,
  pub started: bool,
}

impl Metadata {
  pub async fn read(path: &Path) -> io::Result<Self> {
    let metadata_bytes = fs::read(&path).await?;
    Ok(serde_json::from_slice(&metadata_bytes)?)
  }

  pub async fn write(&self, path: &Path) -> io::Result<()> {
    fs::write(path, serde_json::to_string(self)?).await
  }

  pub async fn modify(path: &Path, f: impl FnOnce(&mut Self)) -> Result<()> {
    let mut metadata: Metadata = serde_json::from_slice(&fs::read(path).await?)?;
    f(&mut metadata);
    fs::write(path, serde_json::to_vec(&metadata)?).await?;
    Ok(())
  }
}
