use crate::{MainState, Result};
use hyper::{Body, Request, Response, StatusCode};
use serde::Serialize;

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

pub(crate) fn authenticate(state: &MainState, req: &Request<Body>) -> bool {
  let result = if let Some(uuid) = state.auth_token {
    (req.headers())
      .get("authorization")
      .map(|x| x == &format!("Abel {uuid}"))
      .unwrap_or(false)
  } else {
    true
  };
  result
}
