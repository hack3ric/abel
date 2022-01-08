mod upload;

use hive_core::path::PathMatcher;
use hive_core::Hive;
use hyper::{Body, Method, Request, Response};
use once_cell::sync::Lazy;
use std::convert::Infallible;

static SERVICES_WITH_NAME: Lazy<PathMatcher> =
  Lazy::new(|| PathMatcher::new("/services/:name").unwrap());

async fn handle_priv(hive: Hive, req: Request<Body>) -> Response<Body> {
  let path = req.uri().path();
  let method = req.method();

  let result = if path == "/" {
    Ok(Response::new("Hello, world!".into()))
  } else if path == "/services" {
    match method {
      &Method::GET => unimplemented!("list"),
      &Method::POST => upload::run(&hive, req, None).await,
      _ => unimplemented!(),
    }
  } else if let Some(params) = SERVICES_WITH_NAME.gen_params(path) {
    match method {
      &Method::GET => unimplemented!("get_service"),
      &Method::PUT => upload::run(&hive, req, Some(params["name"].clone())).await,
      _ => unimplemented!(),
    }
  } else {
    unimplemented!("run_service")
  };

  result.unwrap_or_else(|error| error.into_response(true))
}

pub async fn handle(hive: Hive, req: Request<Body>) -> Result<Response<Body>, Infallible> {
  Ok(handle_priv(hive, req).await)
}
