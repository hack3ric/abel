use backtrace::Backtrace;
use hyper::{Body, Response, StatusCode};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[error("{kind}")]
pub struct Error {
  status: StatusCode,
  kind: ErrorKind,
  backtrace: Option<Backtrace>,
}

#[derive(Debug, thiserror::Error)]
pub enum ErrorKind {
  #[error("{0}")]
  Simple(&'static str),
  #[error(transparent)]
  HiveCore(#[from] hive_core::Error),
  #[error(transparent)]
  Multer(#[from] multer::Error),
}

impl Error {
  pub fn status(&self) -> StatusCode {
    self.status
  }

  pub fn kind(&self) -> &ErrorKind {
    &self.kind
  }

  pub fn backtrace(&self) -> Option<&Backtrace> {
    if let ErrorKind::HiveCore(error) = &self.kind {
      error.backtrace()
    } else {
      self.backtrace.as_ref()
    }
  }

  pub fn into_response(self, authed: bool) -> Response<Body> {
    let body = if self.status.is_server_error() {
      if authed {
        json!({
          "error": "internal server error",
          "detail": self.to_string(),
          "backtrace": self.backtrace().map(|x| format!("{:?}", x)),
        })
      } else {
        json!({ "error": "internal server error" })
      }
    } else {
      json!({ "error": self.to_string() })
    };

    Response::builder()
      .status(self.status)
      .body(body.to_string().into())
      .unwrap()
  }
}

impl<T: TryInto<StatusCode>, U: Into<ErrorKind>> From<(T, U)> for Error {
  fn from((status, kind): (T, U)) -> Self {
    let status = status
      .try_into()
      .map_err(|_| panic!("invalid status code"))
      .unwrap();
    Self {
      status,
      kind: kind.into(),
      backtrace: status.is_server_error().then(Backtrace::new),
    }
  }
}

impl From<&'static str> for Error {
  fn from(msg: &'static str) -> Self {
    (400, msg).into()
  }
}

impl From<hive_core::Error> for Error {
  fn from(error: hive_core::Error) -> Self {
    use hive_core::ErrorKind::*;
    let status = match error.kind() {
      InvalidServiceName(..) => StatusCode::BAD_REQUEST,
      _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    Self {
      status,
      kind: error.into(),
      backtrace: None,
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
    (400, error).into()
  }
}

impl From<&'static str> for ErrorKind {
  fn from(x: &'static str) -> Self {
    Self::Simple(x)
  }
}
