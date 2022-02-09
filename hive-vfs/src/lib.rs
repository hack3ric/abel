mod impls;
mod vfs;

pub use impls::{FileSystem, ReadOnly, ReadOnlyVfs};
pub use vfs::{FileMode, LocalVfs, Metadata, Vfs};

use tokio::io;

/// Result used in this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// VFS error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// An I/O error has been encountered.
  #[error(transparent)]
  Io(#[from] io::Error),

  /// Method is not allowed, e.g. attempting to write on [`ReadOnlyVfs`].
  ///
  /// [`ReadOnlyVfs`]: struct.ReadOnly.html
  #[error("method not allowed")]
  MethodNotAllowed,
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
