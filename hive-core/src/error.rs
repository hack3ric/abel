use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

// TODO: refactor `Error` type to `Error` struct with `ErrorKind` enum and optional backtrace
#[derive(Debug, Error)]
pub enum Error {
  #[error("invalid service name: {0}")]
  InvalidServiceName(Box<str>),

  #[error("service is dropped")]
  ServiceDropped,

  #[error(transparent)]
  Lua(#[from] mlua::Error),
  #[error(transparent)]
  Regex(#[from] regex::Error)
}

impl From<Error> for mlua::Error {
  fn from(x: Error) -> Self {
    match x {
      Error::Lua(source) => source,
      _ => mlua::Error::external(x),
    }
  }
}
