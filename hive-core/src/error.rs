use std::backtrace::Backtrace;
use thiserror::Error;

pub type HiveResult<T> = Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
  #[error("service is dropped")]
  ServiceDropped { backtrace: Backtrace },

  #[error("{source}")]
  Lua {
    #[from]
    source: mlua::Error,
    backtrace: Backtrace,
  },
}

impl From<Error> for mlua::Error {
  fn from(x: Error) -> Self {
    match x {
      Error::Lua { source, .. } => source,
      _ => mlua::Error::external(x),
    }
  }
}
