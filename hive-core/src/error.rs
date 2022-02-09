use backtrace::Backtrace;
use std::fmt::{self, Debug, Formatter};
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Error)]
#[error("{kind}")]
pub struct Error {
  kind: ErrorKind,
  backtrace: Option<Backtrace>,
}

impl Debug for Error {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    Debug::fmt(&self.kind, f)
  }
}

#[derive(Debug, Error)]
pub enum ErrorKind {
  #[error("invalid service name: {0}")]
  InvalidServiceName(Box<str>),
  #[error("service not found: {0}")]
  ServiceNotFound(Box<str>),
  #[error("path not found in service '{service}': {path}")]
  ServicePathNotFound { service: Box<str>, path: Box<str> },
  #[error("service is dropped")]
  ServiceDropped,
  #[error(transparent)]
  Lua(#[from] mlua::Error),
  #[error(transparent)]
  Vfs(#[from] hive_vfs::Error),
  #[error(transparent)]
  Io(#[from] tokio::io::Error),
  #[error(transparent)]
  Regex(#[from] regex::Error),
}

impl Error {
  pub fn kind(&self) -> &ErrorKind {
    &self.kind
  }

  pub fn backtrace(&self) -> Option<&Backtrace> {
    self.backtrace.as_ref()
  }

  pub fn into_backtrace(self) -> Option<Backtrace> {
    self.backtrace
  }
}

impl From<ErrorKind> for Error {
  fn from(kind: ErrorKind) -> Self {
    use ErrorKind::*;
    let backtrace = match kind {
      InvalidServiceName(_) | ServiceNotFound(_) | ServicePathNotFound { .. } => None,
      _ => Some(Backtrace::new()),
    };
    Self { kind, backtrace }
  }
}

// impl From<mlua::Error> for Error {
//   fn from(x: mlua::Error) -> Self {
//     use mlua::Error::*;
//     match x {
//       ExternalError(error) => {
//         error.downcast_ref();
//       }
//       _ => ErrorKind::Lua(error).into()
//     }
//   }
// }

// impl From<Error> for mlua::Error {
//   fn from(x: Error) -> Self {
//     match x.kind {
//       ErrorKind::Lua(source) => source,
//       _ => mlua::Error::external(x),
//     }
//   }
// }

macro_rules! simple_impl_from_errors {
  ($($error:ty),+) => {$(
    impl From<$error> for Error {
      fn from(error: $error) -> Self {
        ErrorKind::from(error).into()
      }
    }
  )+};
}

simple_impl_from_errors!(mlua::Error, regex::Error, hive_vfs::Error, tokio::io::Error);
