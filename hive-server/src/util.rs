use crate::error::ErrorKind::Unauthorized;
use crate::{MainState, Result};
use futures::Future;
use hyper::{Body, Request, Response, StatusCode};
use serde::Serialize;
use tokio::io;
use tokio::task::spawn_blocking;

pub fn json_response(status: StatusCode, body: impl Serialize) -> Result<Response<Body>> {
  Ok(json_response_raw(status, body))
}

pub fn json_response_raw(status: StatusCode, body: impl Serialize) -> Response<Body> {
  Response::builder()
    .status(status)
    .header("content-type", "application/json")
    .body(serde_json::to_string(&body).unwrap().into())
    .unwrap()
}

/// Taken from `tokio::fs`
pub async fn asyncify<F, T, E>(f: F) -> Result<T>
where
  F: FnOnce() -> Result<T, E> + Send + 'static,
  T: Send + 'static,
  E: Send + 'static,
  crate::Error: From<E>,
{
  match spawn_blocking(f).await {
    Ok(res) => res.map_err(From::from),
    Err(_) => Err(io::Error::new(io::ErrorKind::Other, "background task failed").into()),
  }
}

pub(crate) fn authenticate(state: &MainState, req: &Request<Body>) -> bool {
  let result = if let Some(uuid) = state.auth_token {
    (req.headers())
      .get("authentication")
      .map(|x| x == &format!("Hive {uuid}"))
      .unwrap_or(false)
  } else {
    true
  };
  result
}

pub(crate) fn authenticate_ok(result: bool) -> Result<()> {
  if result {
    Ok(())
  } else {
    Err(Unauthorized.into())
  }
}
