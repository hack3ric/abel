mod impls;
mod vfs;

pub use impls::ReadOnlyVfs;
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
