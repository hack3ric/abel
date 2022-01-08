mod upload;

use hive_core::path::PathMatcher;
use hive_core::{Hive, Service};
use hyper::{Body, Method, Request, Response};
use once_cell::sync::Lazy;
use std::convert::Infallible;
use crate::Result;

static SERVICES_WITH_NAME: Lazy<PathMatcher> =
  Lazy::new(|| PathMatcher::new("/services/:name").unwrap());
static SERVICES_WITH_PATH: Lazy<PathMatcher> =
  Lazy::new(|| PathMatcher::new("/services/*").unwrap());

async fn handle_priv(hive: Hive, req: Request<Body>) -> Response<Body> {
  let path = req.uri().path();
  let method = req.method();

  let result = if path == "/" {
    Ok(Response::new("\"Hello, world!\"".into()))
  } else if path == "/services" {
    match method {
      &Method::GET => list(&hive).await,
      &Method::POST => upload::run(&hive, req, None).await,
      _ => unimplemented!(),
    }
  } else if let Some(params) = SERVICES_WITH_NAME.gen_params(path) {
    match method {
      &Method::GET => get_service(&hive, &params["name"]).await,
      &Method::PUT => upload::run(&hive, req, Some(params["name"].clone())).await,
      _ => unimplemented!(),
    }
  } else if let Some(_params) = SERVICES_WITH_PATH.gen_params(path) {
    unimplemented!("/services/*")
  } else {
    unimplemented!("run service")
  };

  result.unwrap_or_else(|error| error.into_response(true))
}

pub async fn handle(hive: Hive, req: Request<Body>) -> Result<Response<Body>, Infallible> {
  Ok(handle_priv(hive, req).await)
}

async fn list(hive: &Hive) -> Result<Response<Body>> {
  let x = hive.list().await;
  let y = x.iter().map(Service::upgrade).collect::<Vec<_>>();
  Ok(Response::new(serde_json::to_string(&y)?.into()))
}

async fn get_service(hive: &Hive, name: &str) -> Result<Response<Body>> {
  match hive.get_service(name).await {
    Some(service) => Ok(Response::new(
      serde_json::to_string(&service.upgrade())?.into(),
    )),
    None => Err((404, format!("service '{}' not found", name)).into()),
  }
}
