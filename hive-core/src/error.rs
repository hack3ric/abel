use std::backtrace::Backtrace;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
  #[error("invalid service name")]
  InvalidServiceName { name: Box<str> },
  // LuaCustom
  #[error("service is dropped")]
  ServiceDropped { backtrace: Backtrace },

  #[error("{source}")]
  Lua {
    #[from]
    source: mlua::Error,
    backtrace: Backtrace,
  },
  #[error("{source}")]
  Regex {
    #[from]
    source: regex::Error,
    backtrace: Backtrace,
  },
  #[error(transparent)]
  Other(#[from] anyhow::Error)
}

impl From<Error> for mlua::Error {
  fn from(x: Error) -> Self {
    match x {
      Error::Lua { source, .. } => source,
      _ => mlua::Error::external(x),
    }
  }
}
