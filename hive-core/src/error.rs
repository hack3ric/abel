use crate::permission::Permission;
use backtrace::Backtrace;
use hyper::{Body, Response, StatusCode};
use serde::Serialize;
use serde_json::json;
use std::fmt::{self, Debug, Formatter};
use strum::EnumProperty;
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

#[derive(Debug, Error, EnumProperty, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ErrorKind {
  // -- Service --
  #[error("invalid service name: {name}")]
  #[strum(props(status = "400", msg = "invalid service name"))]
  InvalidServiceName { name: Box<str> },

  #[error("service '{name}' not found")]
  #[strum(props(status = "404", msg = "service not found"))]
  ServiceNotFound { name: Box<str> },

  #[error("path not found in service '{service}': {path}")]
  #[strum(props(status = "404", msg = "path not found"))]
  ServicePathNotFound { service: Box<str>, path: Box<str> },

  #[error("service '{name}' already exists")]
  #[strum(props(status = "409", msg = "service already exists"))]
  ServiceExists { name: Box<str> },

  #[error("service '{name}' is still running")]
  #[strum(props(status = "409", msg = "service is running"))]
  ServiceRunning { name: Box<str> },

  #[error("service '{name}' is stopped")]
  #[strum(props(status = "409", msg = "service is stopped"))]
  ServiceStopped { name: Box<str> },

  #[error("service is dropped")]
  #[strum(props(status = "500", msg = "service is dropped"))]
  ServiceDropped,

  // -- Permission --
  #[error("permission '{permission}' not granted")]
  #[strum(props(status = "500", msg = "permission not granted"))]
  PermissionNotGranted { permission: Permission<'static> },

  #[error("invalid permission '{string}': {reason}")]
  #[strum(props(status = "500", msg = "invalid permission"))]
  InvalidPermission { string: Box<str>, reason: Box<str> },

  // -- Vendor --
  #[error(transparent)]
  #[serde(skip)]
  #[strum(props(status = "500", msg = "Lua error"))]
  Lua(#[from] mlua::Error),

  #[error(transparent)]
  #[serde(skip)]
  #[strum(props(status = "500", msg = "I/O error"))]
  Io(#[from] tokio::io::Error),

  #[error(transparent)]
  #[serde(skip)]
  #[strum(props(status = "500", msg = "regex error"))]
  Regex(#[from] regex::Error),

  #[error(transparent)]
  #[serde(skip)]
  #[strum(props(status = "500", msg = "hyper error"))]
  Hyper(#[from] hyper::Error),

  // -- Custom --
  #[error("{error} ({detail:?})")]
  #[serde(skip)]
  LuaCustom {
    status: StatusCode,
    error: String,
    detail: serde_json::Value,
  },
}

impl Error {
  pub fn kind(&self) -> &ErrorKind {
    &self.kind
  }

  pub fn into_parts(self) -> (ErrorKind, Option<Backtrace>) {
    (self.kind, self.backtrace)
  }
}

impl From<ErrorKind> for Error {
  fn from(kind: ErrorKind) -> Self {
    use ErrorKind::*;
    let backtrace = match kind {
      InvalidServiceName { .. }
      | ServiceNotFound { .. }
      | ServicePathNotFound { .. }
      | ServiceExists { .. }
      | ServiceRunning { .. }
      | ServiceStopped { .. }
      | LuaCustom { .. } => None,
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
  tokio::io::Error,
  regex::Error,
  hyper::Error,
}

impl From<Error> for Response<Body> {
  fn from(x: Error) -> Self {
    let (status, body) = if let ErrorKind::LuaCustom {
      status,
      error,
      detail,
    } = x.kind
    {
      let body = json!({ "error": error, "detail": detail });
      (status, body)
    } else {
      let status = x.kind.get_str("prop").unwrap().parse().unwrap();
      let error = x.kind.get_str("msg").unwrap();
      let detail = serde_json::to_value(&x.kind).unwrap();
      let body = if let Some(x) = x.backtrace {
        json!({
          "error": error,
          "detail": detail,
          "backtrace": format!("{x:?}"),
        })
      } else {
        json!({
          "error": error,
          "detail": detail,
        })
      };
      (status, body)
    };

    Response::builder()
      .status(status)
      .header("content-type", "application/json")
      .body(body.to_string().into())
      .unwrap()
  }
}
