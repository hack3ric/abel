mod impls;
mod vfs;

pub use impls::{FileSystem, ReadOnly, ReadOnlyVfs};
pub use vfs::{FileMode, LocalVfs, Metadata, Vfs};

use tokio::io;

/// Result used in this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// VFS error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
  /// An I/O error has encountered.
  #[error(transparent)]
  Io(#[from] io::Error),

  #[error("path not found: {0}")]
  NotFound(String),

  /// Method is not allowed, e.g. attempting to write on [`ReadOnlyVfs`].
  ///
  /// [`ReadOnlyVfs`]: struct.ReadOnly.html
  #[error("method not allowed")]
  MethodNotAllowed,

  #[error("'{0}' is not a directory")]
  NotADirectory(String),

  #[error("'{0}' is a directory")]
  IsADirectory(String),
}

impl From<io::ErrorKind> for Error {
  fn from(x: io::ErrorKind) -> Self {
    Self::Io(x.into())
  }
}

pub fn normalize_path(path: &str) -> String {
  let mut result = Vec::new();
  let segments = path.split(['/', '\\']).filter(|&x| x != "" && x != ".");
  for s in segments {
    if s == ".." {
      result.pop();
    } else {
      result.push(s);
    }
  }
  result.join("/")
}

pub trait ResultExt<T> {
  fn to_vfs_err(self, path: &str) -> Result<T>;
}

impl<T> ResultExt<T> for io::Result<T> {
  fn to_vfs_err(self, path: &str) -> Result<T> {
    match self {
      Ok(x) => Ok(x),
      Err(error) => {
        if let io::ErrorKind::NotFound = error.kind() {
          Err(Error::NotFound(path.into()))
        } else {
          Err(Error::Io(error))
        }
      }
    }
  }
}
