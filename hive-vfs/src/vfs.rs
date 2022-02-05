use crate::{Error, Result};
use async_trait::async_trait;
use futures::stream::{BoxStream, LocalBoxStream};
use tokio::io::AsyncRead;

/// Virtual file system interface.
///
/// A virtual file system should at least implement reading functionalities.
#[async_trait]
pub trait Vfs {
  type File: AsyncRead + Send + Sync;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a;

  async fn read_dir(&self, path: &str) -> Result<BoxStream<Result<String>>>;
  async fn metadata(&self, path: &str) -> Result<Metadata>;
  async fn exists(&self, path: &str) -> Result<bool>;

  async fn create_dir(&self, _path: &str) -> Result<()>;
  async fn remove_file(&self, _path: &str) -> Result<()>;
  async fn remove_dir(&self, _path: &str) -> Result<()>;
}

/// Virtual file system interface for non-`Send` types.
///
/// See [`Vfs`] for more documentation.
///
/// [`Vfs`]: trait.Vfs.html
#[async_trait(?Send)]
pub trait LocalVfs {
  type File: AsyncRead;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a;

  async fn read_dir(&self, path: &str) -> Result<LocalBoxStream<Result<String>>>;
  async fn metadata(&self, path: &str) -> Result<Metadata>;
  async fn exists(&self, path: &str) -> Result<bool>;

  async fn create_dir(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn remove_file(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }
}

#[async_trait(?Send)]
impl<T: Vfs + Sync> LocalVfs for T {
  type File = T::File;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a,
  {
    self.open_file(path, mode).await
  }

  async fn create_dir(&self, path: &str) -> Result<()> {
    self.create_dir(path).await
  }

  async fn read_dir(&self, path: &str) -> Result<LocalBoxStream<Result<String>>> {
    Ok(self.read_dir(path).await?)
  }

  async fn metadata(&self, path: &str) -> Result<Metadata> {
    self.metadata(path).await
  }

  async fn exists(&self, path: &str) -> Result<bool> {
    self.exists(path).await
  }

  async fn remove_file(&self, path: &str) -> Result<()> {
    self.remove_file(path).await
  }

  async fn remove_dir(&self, path: &str) -> Result<()> {
    self.remove_dir(path).await
  }
}

/// Modes for opening a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
  /// Read-only mode. Corresponds to `r` in C's `fopen`.
  Read,

  /// Write-only mode. Corresponds to `w` in C's `fopen`.
  Write,

  /// Append mode. Corresponds to `a` in C's `fopen`.
  Append,

  /// Read-and-write mode, preserving original data. Corresponds to `r+` in C's
  /// `fopen`.
  ReadWrite,

  /// Read-and-write mode, removing original data. Corresponds to `w+` in C's
  /// `fopen`.
  ReadWriteNew,

  /// Read-and-append mode,. Corresponds to `a+` in C's `fopen`.
  ReadAppend,
}

/// Information of an entry in a VFS.
#[derive(Debug, Clone, Copy)]
pub enum Metadata {
  /// Indicates the entry is a file.
  File {
    /// Size of the file.
    len: u64 
  },

  /// Indicates the entry is a directory.
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