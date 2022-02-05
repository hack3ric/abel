use std::sync::Arc;
use hive_vfs::{Vfs, FileMode, Metadata};
use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt};
use std::pin::Pin;
use futures::stream::BoxStream;
use std::fmt::Debug;
use std::borrow::Cow;
use crate::Result;

/// Shared, immutable source code storage.
#[derive(Debug, Clone)]
pub struct Source {
  vfs: Pin<Arc<dyn DebugVfs<File = Pin<Box<dyn AsyncRead + Send + Sync>>> + Send + Sync>>
  // TODO: cache
}

trait DebugVfs: Debug + Vfs {}
impl<T: Debug + Vfs> DebugVfs for T {}

impl Source {
  pub fn new<T>(vfs: T) -> Self
  where
    T: Vfs + Debug + Send + Sync + 'static,
    T::File: Send + Sync + 'static,
  {
    Self {
      vfs: Arc::pin(ReadVfs(vfs))
    }
  }

  pub(crate) async fn get(&self, path: &str) -> Result<Cow<'_, [u8]>> {
    let mut f = self.vfs.open_file(path, FileMode::Read).await?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).await?;
    Ok(buf.into())
  }
}

#[derive(Debug)]
struct ReadVfs<T: Vfs + Send + Sync>(T)
where
  T::File: 'static;

#[async_trait]
impl<T: Vfs + Send + Sync> Vfs for ReadVfs<T>
where
  T::File: 'static
{
  type File = Pin<Box<dyn AsyncRead + Send + Sync>>;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> hive_vfs::Result<Self::File>
  where
    Self::File: 'a
  {
    Ok(Box::pin(self.0.open_file(path, mode).await?))
  }

  async fn read_dir(&self, path: &str) ->hive_vfs::Result<BoxStream<hive_vfs::Result<String>>> {
    self.0.read_dir(path).await
  }

  async fn metadata(&self, path: &str) -> hive_vfs::Result<Metadata> {
    self.0.metadata(path).await
  }

  async fn exists(&self, path: &str) -> hive_vfs::Result<bool> {
    self.0.exists(path).await
  }

  async fn create_dir(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }

  async fn remove_file(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }
}
