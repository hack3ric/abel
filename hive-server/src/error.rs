use crate::util::json_response;
use backtrace::Backtrace;
use hyper::{Body, Method, Response, StatusCode};
use serde_json::json;
use serde_json::Value::String as JsonString;
use std::borrow::Cow;

#[derive(Debug, thiserror::Error)]
#[error("{error} ({detail})")]
pub struct Error {
  status: StatusCode,
  error: Cow<'static, str>,
  detail: serde_json::Value,
  backtrace: Option<Backtrace>,
}

impl Error {
  #[allow(unused)]
  pub fn backtrace(&self) -> Option<&Backtrace> {
    self.backtrace.as_ref()
  }

  pub fn into_response(self, authed: bool) -> Response<Body> {
    let body = if self.status.is_server_error() {
      if authed {
        json!({
          "error": self.error,
          "detail": self.detail,
          // "backtrace": self.backtrace().map(|x| format!("{:?}", x)),
        })
      } else {
        json!({ "error": "internal server error" })
      }
    } else {
      json!({
        "error": self.error,
        "detail": self.detail
      })
    };

    json_response(self.status, body)
  }
}

impl<T, U, V> From<(T, U, V)> for Error
where
  T: TryInto<StatusCode>,
  U: Into<Cow<'static, str>>,
  V: Into<serde_json::Value>,
{
  fn from((status, error, detail): (T, U, V)) -> Self {
    let status = status
      .try_into()
      .map_err(|_| panic!("invalid status code"))
      .unwrap();
    Self {
      status,
      error: error.into(),
      detail: detail.into(),
      backtrace: status.is_server_error().then(Backtrace::new),
    }
  }
}

impl From<&'static str> for Error {
  fn from(msg: &'static str) -> Self {
    (400, msg, serde_json::Value::Null).into()
  }
}

impl From<hive_core::Error> for Error {
  fn from(error: hive_core::Error) -> Self {
    use hive_core::ErrorKind::*;
    let (status, error_, detail) = match error.kind() {
      InvalidServiceName(name) => (400, "invalid service name", json!({ "name": name })),
      ServiceNotFound(name) => (404, "service not found", json!({ "name": name })),
      PathNotFound { service, path } => (
        404,
        "service found but path not found",
        json!({ "service": service, "path": path }),
      ),
      Lua(error) => (500, "Lua error", JsonString(error.to_string())),
      _ => (500, "hive core error", JsonString(error.to_string())),
    };
    Self {
      status: status.try_into().unwrap(),
      error: error_.into(),
      detail,
      backtrace: error.into_backtrace(),
    }
  }
}

// Errors when reading multipart body are *mostly* client-side, so they all
// currently use 400 Bad Request for simplicity.
//
// This may change in the future if `multer::Error` proved not suitable to
// be exposed to untrusted client.
impl From<multer::Error> for Error {
  fn from(error: multer::Error) -> Self {
    (400, "failed to read multipart body", error.to_string()).into()
  }
}

impl From<serde_json::Error> for Error {
  fn from(error: serde_json::Error) -> Self {
    (500, "failed to (de)serialize object", error.to_string()).into()
  }
}
impl From<serde_qs::Error> for Error {
  fn from(error: serde_qs::Error) -> Self {
    (500, "failed to (de)serialize object", error.to_string()).into()
  }
}

impl From<tokio::io::Error> for Error {
  fn from(error: tokio::io::Error) -> Self {
    (500, "I/O error", error.to_string()).into()
  }
}

pub fn method_not_allowed(expected: &[&'static str], got: &Method) -> Error {
  From::from((
    405,
    "method not allowed",
    json!({ "expected": expected, "got": got.as_str() }),
  ))
}
