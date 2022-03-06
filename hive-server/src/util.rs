use crate::Result;
use hyper::{Body, Response, StatusCode};
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
