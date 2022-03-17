use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use uuid::Uuid;

/// Extra information of a loaded service.
#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
  pub uuid: Uuid,
  pub started: bool,
}

pub async fn modify_metadata(path: &Path, f: impl FnOnce(&mut Metadata)) -> Result<()> {
  let mut metadata: Metadata = serde_json::from_slice(&fs::read(path).await?)?;
  f(&mut metadata);
  fs::write(path, serde_json::to_vec(&metadata)?).await?;
  Ok(())
}
