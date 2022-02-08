use hyper::{Body, Response, StatusCode};
use serde::Serialize;
use std::collections::HashMap;

pub fn json_response(status: StatusCode, body: impl Serialize) -> Response<Body> {
  Response::builder()
    .status(status)
    .header("Content-Type", "application/json")
    .body(serde_json::to_string(&body).unwrap().into())
    .unwrap()
}
