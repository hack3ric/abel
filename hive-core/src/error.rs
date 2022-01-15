use backtrace::Backtrace;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
#[error("{kind}")]
pub struct Error {
  kind: ErrorKind,
  backtrace: Option<Backtrace>,
}

#[derive(Debug, Error)]
pub enum ErrorKind {
  #[error("invalid service name: {0}")]
  InvalidServiceName(Box<str>),
  #[error("service not found: {0}")]
  ServiceNotFound(Box<str>),
  #[error("path not found in service '{service}': {path}")]
  PathNotFound { service: Box<str>, path: Box<str> },

  #[error("service is dropped")]
  ServiceDropped,

  #[error(transparent)]
  Lua(#[from] mlua::Error),
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
      InvalidServiceName(_) | ServiceNotFound(_) | PathNotFound { .. } => None,
      _ => Some(Backtrace::new()),
    };
    Self { kind, backtrace }
  }
}

macro_rules! simple_impl_from_errors {
  ($($error:ty),+) => {$(
    impl From<$error> for Error {
      fn from(error: $error) -> Self {
        ErrorKind::from(error).into()
      }
    }
  )+};
}

simple_impl_from_errors!(mlua::Error, regex::Error);

// impl From<Error> for mlua::Error {
//   fn from(x: Error) -> Self {
//     match x {
//       Error::Lua(source) => source,
//       _ => mlua::Error::external(x),
//     }
//   }
// }
