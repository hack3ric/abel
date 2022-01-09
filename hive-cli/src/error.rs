use backtrace::Backtrace;
use hyper::{Body, Method, Response, StatusCode};
use serde_json::json;
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
  pub fn backtrace(&self) -> Option<&Backtrace> {
    self.backtrace.as_ref()
  }

  pub fn into_response(self, authed: bool) -> Response<Body> {
    let body = if self.status.is_server_error() {
      if authed {
        json!({
          "error": self.error,
          "detail": self.detail,
          "backtrace": self.backtrace().map(|x| format!("{:?}", x)),
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

    Response::builder()
      .status(self.status)
      .body(body.to_string().into())
      .unwrap()
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
      Lua(error) => (500, "Lua error", simple_msg(error)),
      _ => (500, "hive core error", simple_msg(&error)),
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
    (400, "failed to read multipart body", simple_msg(error)).into()
  }
}

impl From<serde_json::Error> for Error {
  fn from(error: serde_json::Error) -> Self {
    (500, "failed to (de)serialize object", simple_msg(error)).into()
  }
}

fn simple_msg(x: impl ToString) -> serde_json::Value {
  json!({ "msg": x.to_string() })
}

pub fn method_not_allowed(expected: &[&'static str], got: &Method) -> Error {
  (
    405,
    "method not allowed",
    json!({ "expected": expected, "got": got.as_str() }),
  )
    .into()
}
