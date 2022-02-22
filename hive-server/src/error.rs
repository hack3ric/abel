use crate::util::json_response_raw;
use backtrace::Backtrace;
use hyper::{Body, Method, Response, StatusCode};
use serde_json::json;
use serde_json::Value::{Object as JsonObject, String as JsonString};
use std::borrow::Cow;
use std::fmt::{self, Debug, Display, Formatter};

#[derive(thiserror::Error)]
pub struct Error {
  status: StatusCode,
  error: Cow<'static, str>,
  detail: serde_json::Value,
  backtrace: Option<Backtrace>,
}

impl Debug for Error {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("Error")
      .field("status", &self.status)
      .field("error", &self.error)
      .field("detail", &self.detail)
      .finish_non_exhaustive()
  }
}

impl Display for Error {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.write_str(&self.error)?;
    f.write_str(": ")?;
    match &self.detail {
      serde_json::Value::Object(map) => {
        if map.len() == 1 {
          if let Some(serde_json::Value::String(s)) = map.get("msg") {
            return f.write_str(s);
          }
        }
        f.write_str(&serde_json::to_string_pretty(map).unwrap())
      }
      serde_json::Value::String(s) => f.write_str(s),
      _ => f.write_str(&self.detail.to_string()),
    }
  }
}

impl Error {
  pub fn add_detail(&mut self, key: String, info: serde_json::Value) {
    match &mut self.detail {
      JsonObject(map) => {
        map.insert(key, info);
      }
      detail => {
        let mut map = JsonObject(serde_json::Map::new());
        std::mem::swap(&mut map, detail);
        self.add_detail("msg".to_string(), map);
        self.add_detail(key, info);
      }
    }
  }

  #[allow(unused)]
  pub fn backtrace(&self) -> Option<&Backtrace> {
    self.backtrace.as_ref()
  }

  pub fn into_response(self, authed: bool) -> Response<Body> {
    let use_backtrace = option_env!("RUST_BACKTRACE").is_some();
    let body = if self.status.is_server_error() {
      // TODO: include UUID
      if authed {
        json!({
          "error": self.error,
          "detail": self.detail,
          "backtrace": use_backtrace
            .then(|| self.backtrace().map(|x| format!("{:?}", x))),
        })
      } else {
        json!({
          "error": "internal server error",
          "detail": {
            "msg": "Contact system administrator for help"
          }
        })
      }
    } else {
      json!({
        "error": self.error,
        "detail": self.detail
      })
    };

    json_response_raw(self.status, body)
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

    let detail = match detail.into() {
      serde_json::Value::String(s) => json!({ "msg": s }),
      other => other,
    };

    Self {
      status,
      error: error.into(),
      detail,
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
      ServicePathNotFound { service, path } => (
        404,
        "path not found in service",
        json!({ "service": service, "path": path }),
      ),
      Lua(error) => (500, "Lua error", JsonString(error.to_string())),
      Vfs(error) => {
        if let hive_vfs::Error::NotFound(path) = error {
          (400, "VFS path not found", json!({ "path": path }))
        } else {
          (500, "hive core error", JsonString(error.to_string()))
        }
      }
      _ => (500, "hive core error", JsonString(error.to_string())),
    };
    Self {
      backtrace: error.into_backtrace(),
      ..(status, error_, detail).into()
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
