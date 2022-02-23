use crate::permission::Permission;
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
  #[error("service '{0}' not found")]
  ServiceNotFound(Box<str>),
  #[error("path not found in service '{service}': {path}")]
  ServicePathNotFound { service: Box<str>, path: Box<str> },
  #[error("service is dropped")]
  ServiceDropped,
  #[error("permission '{0}' not granted")]
  PermissionNotGranted(Permission),

  #[error(transparent)]
  Lua(#[from] mlua::Error),
  #[error(transparent)]
  Vfs(#[from] hive_vfs::Error),
  #[error(transparent)]
  Io(#[from] tokio::io::Error),
  #[error(transparent)]
  Regex(#[from] regex::Error),
  #[error(transparent)]
  Hyper(#[from] hyper::Error),
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

impl From<Error> for mlua::Error {
  fn from(x: Error) -> Self {
    if let ErrorKind::Lua(x) = x.kind {
      x
    } else {
      mlua::Error::external(x)
    }
  }
}

macro_rules! simple_impl_from_errors {
  ($($error:ty),+$(,)?) => {$(
    impl From<$error> for Error {
      fn from(error: $error) -> Self {
        ErrorKind::from(error).into()
      }
    }
  )+};
}

simple_impl_from_errors! {
  mlua::Error,
  hive_vfs::Error,
  tokio::io::Error,
  regex::Error,
  hyper::Error,
}
