use async_trait::async_trait;
use tokio::io::{self, AsyncRead, AsyncSeek, AsyncWrite};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[async_trait(?Send)]
pub trait Vfs {
  type File: AsyncRead + AsyncWrite + AsyncSeek;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a;
  async fn create_dir(&self, path: &str) -> Result<()>;
  async fn read_dir(&self, path: &str) -> Result<Box<dyn Iterator<Item = String>>>;
  async fn metadata(&self, path: &str) -> Result<Metadata>;
  async fn exists(&self, path: &str) -> Result<bool>;
  async fn remove_file(&self, path: &str) -> Result<()>;
  async fn remove_dir(&self, path: &str) -> Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
  /// Read-only mode. Corresponds to `r` in C's `fopen`.
  Read,
  /// Write-only mode. Corresponds to `w` in C's `fopen`.
  Write,
  /// Append mode. Corresponds to `a` in C's `fopen`.
  Append,
  /// Read-and-write mode, preserving original data. Corresponds to `r+` in C's `fopen`.
  ReadWrite,
  /// Read-and-write mode, removing original data. Corresponds to `w+` in C's `fopen`.
  ReadWriteNew,
  /// Read-and-append mode,. Corresponds to `a+` in C's `fopen`.
  ReadAppend,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// An I/O error has been encountered.
  #[error(transparent)]
  Io(#[from] io::Error),
  /// Method is not allowed, e. g. attempting to write on [`ReadOnly`].
  /// 
  /// [`ReadOnly`]: struct.ReadOnly.html
  #[error("method not allowed")]
  MethodNotAllowed,
}

#[derive(Debug, Clone, Copy)]
pub enum Metadata {
  File { len: u64 },
  Directory,
}

impl Metadata {
  pub fn is_file(&self) -> bool {
    matches!(self, Self::File { .. })
  }

  pub fn is_dir(&self) -> bool {
    matches!(self, Self::Directory)
  }
}

pub struct ReadOnly<T: Vfs>(T);

#[async_trait(?Send)]
impl<T: Vfs> Vfs for ReadOnly<T> {
  type File = T::File;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a
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

  async fn remove_file(&self, path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }
}
