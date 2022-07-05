use abel_core::path::normalize_path_str;
use abel_core::source::SourceVfs;
use async_trait::async_trait;
use hive_asar::{Archive, DuplicableFile};
use std::io::Cursor;
use std::sync::Arc;
use tokio::io;

pub struct AsarSource(pub(crate) Archive<DuplicableFile>);

#[async_trait]
impl SourceVfs for AsarSource {
  type File = hive_asar::File<DuplicableFile>;

  async fn get(&self, path: &str) -> io::Result<Self::File> {
    self.0.get_owned(path).await
  }

  async fn exists(&self, path: &str) -> io::Result<bool> {
    Ok(self.0.get_entry(path).is_some())
  }
}

pub struct SingleSource(Arc<[u8]>);

impl SingleSource {
  pub fn new(src: impl AsRef<[u8]>) -> Self {
    Self(Arc::from(src.as_ref()))
  }
}

#[async_trait]
impl SourceVfs for SingleSource {
  type File = Cursor<Arc<[u8]>>;

  async fn get(&self, path: &str) -> io::Result<Self::File> {
    match &*normalize_path_str(path) {
      "main.lua" => Ok(Cursor::new(self.0.clone())),
      "" => Err(io::Error::from_raw_os_error(libc::EISDIR)),
      _ => Err(io::Error::from_raw_os_error(libc::ENOENT)),
    }
  }

  async fn exists(&self, path: &str) -> io::Result<bool> {
    match &*normalize_path_str(path) {
      "main.lua" | "" => Ok(true),
      _ => Ok(false),
    }
  }
}
