mod upload;

use hive_core::path::PathMatcher;
use hive_core::{Hive, Result};
use hyper::{Body, Method, Request, Response};
use std::lazy::SyncLazy;

static SERVICES_WITH_NAME: SyncLazy<PathMatcher> =
  SyncLazy::new(|| PathMatcher::new("/services/:name").unwrap());

pub async fn handle(hive: Hive, req: Request<Body>) -> Result<Response<Body>> {
  let path = req.uri().path();
  let method = req.method();

  if path == "/" {
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
  }
}
