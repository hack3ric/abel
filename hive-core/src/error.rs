use erased_serde::Serialize;
use hyper::StatusCode;
use serde_json::json;
use std::backtrace::Backtrace;
use std::error::Error as StdError;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
  #[error("invalid service name")]
  InvalidServiceName { name: Box<str> },

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
}

impl From<Error> for mlua::Error {
  fn from(x: Error) -> Self {
    match x {
      Error::Lua { source, .. } => source,
      _ => mlua::Error::external(x),
    }
  }
}

pub struct RawError {
  pub status: StatusCode,
  pub code: Box<str>,
  pub msg: Box<str>,
  pub detail: Box<dyn Serialize>,
  pub backtrace: Option<Box<str>>,
  pub sensitive: bool,
}

impl From<Error> for RawError {
  fn from(x: Error) -> Self {
    use Error::*;
    let (status, code, msg, detail, backtrace, sensitive) = match x {
      InvalidServiceName { name } => (
        StatusCode::BAD_REQUEST,
        "hive::INVALID_SERVICE_NAME",
        "service name is invalid",
        Box::new(json!({ "name": name })) as Box<dyn Serialize>,
        None,
        false,
      ),
      _ => (
        StatusCode::INTERNAL_SERVER_ERROR,
        "hive::INTERNAL_SERVER_ERROR",
        "internal server error",
        Box::new(x.to_string()) as Box<dyn Serialize>,
        x.backtrace(),
        true,
      ),
    };
    Self {
      status,
      code: code.into(),
      msg: msg.into(),
      detail,
      backtrace: backtrace.map(|x| x.to_string().into()),
      sensitive,
    }
  }
}

#[cfg(feature = "multer")]
impl From<multer::Error> for RawError {
  fn from(x: multer::Error) -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      code: "hive::MULTIPART_ERROR".into(),
      msg: "error reading multipart body".into(),
      detail: Box::new(x.to_string()),
      backtrace: None,
      sensitive: true,
    }
  }
}
