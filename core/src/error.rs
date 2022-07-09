use crate::service::ServiceName;
use abel_rt::{mlua, CustomError};
use backtrace::Backtrace;
use hyper::StatusCode;
use serde::{Serialize, Serializer};
use serde_json::json;
use std::fmt::Debug;
use strum::EnumProperty;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
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

impl<E: Into<ErrorKind>> From<E> for Error {
  fn from(x: E) -> Self {
    let kind = x.into();
    // let backtrace = if kind.internal() {
    //   Some(Backtrace::new())
    // } else {
    //   None
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
  #[strum(props(status = "400", error = "invalid service name"))]
  InvalidServiceName { name: ServiceName },

  #[error("service '{name}' not found")]
  #[strum(props(status = "404", error = "service not found"))]
  ServiceNotFound { name: ServiceName },

  #[error("path not found in service '{service}': {path}")]
  #[strum(props(status = "404", error = "path not found"))]
  ServicePathNotFound {
    service: ServiceName,
    path: Box<str>,
  },

  #[error("service '{name}' already exists")]
  #[strum(props(status = "409", error = "service already exists"))]
  ServiceExists { name: ServiceName },

  #[error("service '{name}' is still running")]
  #[strum(props(status = "409", error = "service is running"))]
  ServiceRunning { name: ServiceName },

  #[error("service '{name}' is stopped")]
  #[strum(props(status = "409", error = "service is stopped"))]
  ServiceStopped { name: ServiceName },

  #[error("service is dropped")]
  #[strum(props(status = "500", error = "service is dropped"))]
  ServiceDropped,

  // -- Vendor --
  #[error(transparent)]
  #[strum(props(status = "500", error = "Lua error"))]
  Lua(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    mlua::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "500", error = "I/O error"))]
  Io(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    tokio::io::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "500", error = "regex error"))]
  Regex(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    regex::Error,
  ),

  // -- Custom --
  #[error("{0}")]
  #[serde(skip)]
  Custom(CustomError),
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
      Self::Custom(CustomError { status, .. }) => *status,
      _ => self.get_str("status").unwrap().parse().unwrap(),
    }
  }

  pub fn error(&self) -> &str {
    match self {
      Self::Custom(CustomError { error, .. }) => error,
      _ => self.get_str("error").unwrap(),
    }
  }

  pub fn detail(&self) -> serde_json::Value {
    match self {
      Self::Custom(CustomError { detail, .. }) => detail.clone(),
      _ => serde_json::to_value(self).unwrap(),
    }
  }

  pub fn internal(&self) -> bool {
    match self {
      Self::Custom { .. } => false,
      _ => self.status().is_server_error(),
    }
  }
}
