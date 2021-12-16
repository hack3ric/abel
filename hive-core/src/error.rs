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
