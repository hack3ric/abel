use crate::util::json_response_raw;
use backtrace::Backtrace;
use hyper::{Body, Method, Response, StatusCode};
use serde::{Serialize, Serializer};
use serde_json::json;
use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use strum::EnumProperty;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub struct Error {
  kind: ErrorKind,
  detail: Option<serde_json::Map<String, serde_json::Value>>,
  backtrace: Option<Backtrace>,
}

impl Error {
  pub fn add_detail(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
    if self.detail.is_none() {
      self.detail = Some(Default::default());
    }
    if let Some(detail) = &mut self.detail {
      detail.insert(key.into(), value.into());
    } else {
      unreachable!()
    }
  }

  pub fn kind(&self) -> &ErrorKind {
    &self.kind
  }

  pub fn into_status_and_body(self) -> (StatusCode, serde_json::Map<String, serde_json::Value>) {
    use ErrorKind::*;
    let (status, error, detail, backtrace) = match self.kind {
      Abel(x) => {
        let (kind, backtrace) = x.into_parts();
        (
          kind.status(),
          kind.error().to_string().into(),
          kind.detail(),
          backtrace,
        )
      }
      Custom {
        status,
        error,
        detail,
      } => (status, error, detail, self.backtrace),
      _ => (
        self.kind.get_str("status").unwrap().parse().unwrap(),
        self.kind.get_str("error").unwrap().into(),
        serde_json::to_value(&self.kind).unwrap(),
        self.backtrace,
      ),
    };

    let detail: Option<serde_json::Map<_, _>> = match detail {
      serde_json::Value::Null => self.detail,
      serde_json::Value::Object(mut o) => {
        if let Some(d) = self.detail {
          o.extend(d);
        }
        Some(o)
      }
      serde_json::Value::String(s) => {
        let mut o = serde_json::Map::new();
        o.insert("msg".into(), s.into());
        if let Some(d) = self.detail {
          o.extend(d);
        }
        Some(o)
      }
      _ => panic!("expected null, string or object as error detail"),
    };

    let mut body = serde_json::Map::<String, serde_json::Value>::new();
    body.insert("error".to_string(), error.into_owned().into());
    if let Some(detail) = detail {
      body.insert("detail".to_string(), detail.into());
    }
    if let Some(bt) = backtrace {
      body.insert("backtrace".to_string(), format!("{bt:?}").into());
    }

    (status, body)
  }
}

impl<E: Into<ErrorKind>> From<E> for Error {
  fn from(x: E) -> Self {
    Self {
      kind: x.into(),
      detail: None,
      // TODO: Add backtrace for some error kind
      backtrace: None,
    }
  }
}

impl From<abel_core::ErrorKind> for Error {
  fn from(x: abel_core::ErrorKind) -> Self {
    Self {
      kind: ErrorKind::Abel(x.into()),
      detail: None,
      backtrace: None,
    }
  }
}

impl Display for Error {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    self.kind.fmt(f)?;
    if let Some(detail) = &self.detail {
      write!(f, " ({detail:?})")?;
    }
    Ok(())
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
      kind: ErrorKind::Custom {
        status,
        error: error.into(),
        detail,
      },
      detail: None,
      backtrace: status.is_server_error().then(Backtrace::new),
    }
  }
}

impl<T, U> From<(T, U)> for Error
where
  T: Into<Cow<'static, str>>,
  U: Into<serde_json::Value>,
{
  fn from((error, detail): (T, U)) -> Self {
    (400, error, detail).into()
  }
}

impl From<&'static str> for Error {
  fn from(msg: &'static str) -> Self {
    (400, msg, serde_json::Value::Null).into()
  }
}

impl From<Error> for Response<Body> {
  fn from(x: Error) -> Self {
    let (status, body) = x.into_status_and_body();

    json_response_raw(status, body)
  }
}

#[derive(Debug, thiserror::Error, EnumProperty, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ErrorKind {
  #[error("unauthorized")]
  #[strum(props(status = "401", error = "unauthorized"))]
  Unauthorized,

  // Errors when reading multipart body are *mostly* client-side, so they all
  // currently use 400 Bad Request for simplicity.
  //
  // This may change in the future if `multer::Error` proved not suitable to
  // be exposed to untrusted client.
  #[error(transparent)]
  #[strum(props(status = "400", error = "failed to read multipart body"))]
  Multipart(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    multer::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "400", error = "failed to (de)serialize object"))]
  SerdeJson(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    serde_json::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "400", error = "failed to parse query string"))]
  SerdeQs(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    serde_qs::Error,
  ),

  #[error(transparent)]
  #[strum(props(status = "500", error = "I/O error"))]
  Io(
    #[from]
    #[serde(serialize_with = "serialize_error")]
    tokio::io::Error,
  ),

  #[error(transparent)]
  #[serde(skip)]
  Abel(#[from] abel_core::Error),

  #[error("{error}: {detail:?}")]
  #[serde(skip)]
  Custom {
    status: StatusCode,
    error: Cow<'static, str>,
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
      ErrorKind::Abel(error) => error.kind().status(),
      ErrorKind::Custom { status, .. } => *status,
      _ => self.get_str("status").unwrap().parse().unwrap(),
    }
  }

  pub fn internal(&self) -> bool {
    match self {
      ErrorKind::Abel(error) => error.kind().internal(),
      _ => self.status().is_server_error(),
    }
  }
}

pub fn method_not_allowed(expected: &[&'static str], got: &Method) -> Error {
  From::from((
    405,
    "method not allowed",
    json!({ "expected": expected, "got": got.as_str() }),
  ))
}

#[derive(Debug, thiserror::Error)]
pub struct ErrorAuthWrapper {
  inner: Error,
  uuid: Option<Uuid>,
}

impl ErrorAuthWrapper {
  pub fn new(auth: bool, error: impl Into<Error>) -> Self {
    let inner = error.into();
    let uuid = if !auth && inner.kind.internal() {
      Some(Uuid::new_v4())
    } else {
      None
    };
    Self { inner, uuid }
  }

  pub fn uuid(&self) -> Option<Uuid> {
    self.uuid
  }
}

impl Display for ErrorAuthWrapper {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    self.inner.fmt(f)
  }
}

impl From<ErrorAuthWrapper> for Response<Body> {
  // Hide internal error here
  fn from(error: ErrorAuthWrapper) -> Self {
    if let Some(uuid) = error.uuid {
      json_response_raw(
        error.inner.kind.status(),
        json!({
          "error": "internal error",
          "detail": {
            "msg": "contact system administrator for help",
            "uuid": uuid
          }
        }),
      )
    } else {
      error.inner.into()
    }
  }
}
