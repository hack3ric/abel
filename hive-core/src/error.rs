use crate::permission::Permission;
use backtrace::Backtrace;
use hyper::StatusCode;
use serde::{Serialize, Serializer};
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

impl Error {
  pub fn kind(&self) -> &ErrorKind {
    &self.kind
  }

  pub fn into_parts(self) -> (ErrorKind, Option<Backtrace>) {
    (self.kind, self.backtrace)
  }
}

impl Debug for Error {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    Debug::fmt(&self.kind, f)
  }
}

impl<E: Into<ErrorKind>> From<E> for Error {
  fn from(x: E) -> Self {
    // use ErrorKind::*;
    let kind = x.into();
    // let backtrace = match kind {
    //   InvalidServiceName { .. }
    //   | ServiceNotFound { .. }
    //   | ServicePathNotFound { .. }
    //   | ServiceExists { .. }
    //   | ServiceRunning { .. }
    //   | ServiceStopped { .. }
    //   | LuaCustom { .. } => None,
    //   _ => Some(Backtrace::new()),
    // };
    let backtrace = None;
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
  #[strum(props(status = "500", msg = "Lua error"))]
  Lua(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    mlua::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "500", msg = "I/O error"))]
  Io(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    tokio::io::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "500", msg = "regex error"))]
  Regex(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    regex::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "500", msg = "hyper error"))]
  Hyper(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    hyper::Error,
  ),

  // -- Custom --
  #[error("{msg} ({detail:?})")]
  #[serde(skip)]
  LuaCustom {
    status: StatusCode,
    msg: String,
    detail: serde_json::Value,
  },
}

fn serialize_error<E, S>(error: E, ser: S) -> Result<S::Ok, S::Error>
where
  E: std::error::Error,
  S: Serializer,
{
  json!({ "msg": error.to_string() }).serialize(ser)
}

impl ErrorKind {
  pub fn status(&self) -> StatusCode {
    match self {
      Self::LuaCustom { status, .. } => *status,
      _ => self.get_str("status").unwrap().parse().unwrap(),
    }
  }

  pub fn msg(&self) -> &str {
    match self {
      Self::LuaCustom { msg, .. } => msg,
      _ => self.get_str("msg").unwrap(),
    }
  }

  pub fn detail(&self) -> serde_json::Value {
    match self {
      Self::LuaCustom { detail, .. } => detail.clone(),
      _ => serde_json::to_value(self).unwrap(),
    }
  }
}
