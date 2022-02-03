use crate::{Error, FileMode, Metadata, Result, Vfs};
use async_trait::async_trait;

/// Wrapper type that makes an VFS read-only.
pub struct ReadOnly<T: Vfs + Send + Sync>(T);

#[async_trait]
impl<T: Vfs + Send + Sync> Vfs for ReadOnly<T> {
  type File = T::File;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a,
  {
    if let FileMode::Read = mode {
      self.0.open_file(path, mode).await
    } else {
      Err(Error::MethodNotAllowed)
    }
  }

  async fn create_dir(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn read_dir(&self, path: &str) -> Result<Box<dyn Iterator<Item = String>>> {
    self.0.read_dir(path).await
  }

  async fn metadata(&self, path: &str) -> Result<Metadata> {
    self.0.metadata(path).await
  }

  async fn exists(&self, path: &str) -> Result<bool> {
    self.0.exists(path).await
  }

  async fn remove_file(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }
}
