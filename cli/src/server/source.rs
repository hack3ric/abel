use abel_rt::{normalize_path_str, Metadata, SourceVfs};
use async_trait::async_trait;
use hive_asar::header::Entry;
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

  async fn metadata(&self, path: &str) -> io::Result<Metadata> {
    let entry = (self.0)
      .get_entry(path)
      .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No such file or directory"))?;
    match entry {
      Entry::Directory(_) => Ok(Metadata::Dir),
      Entry::File(m) => Ok(Metadata::File { size: m.size }),
    }
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
      _ => Err(io::Error::new(
        io::ErrorKind::NotFound,
        "No such file or directory",
      )),
    }
  }

  async fn exists(&self, path: &str) -> io::Result<bool> {
    match &*normalize_path_str(path) {
      "main.lua" | "" => Ok(true),
      _ => Ok(false),
    }
  }

  async fn metadata(&self, path: &str) -> io::Result<Metadata> {
    match &*normalize_path_str(path) {
      "main.lua" => Ok(Metadata::File {
        size: self.0.len() as _,
      }),
      "" => Ok(Metadata::Dir),
      _ => Err(io::Error::new(
        io::ErrorKind::NotFound,
        "No such file or directory",
      )),
    }
  }
}
