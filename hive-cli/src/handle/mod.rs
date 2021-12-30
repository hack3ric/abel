use hive_core::path::PathMatcher;
use hive_core::{Hive, Result, RawError, Source};
use hyper::{Body, Method, Request, Response};
use once_cell::sync::Lazy;
use multer::{Constraints, SizeLimit, Multipart};
use std::convert::Infallible;

static SERVICES_WITH_NAME: Lazy<PathMatcher> =
  Lazy::new(|| PathMatcher::new("/services/:name").unwrap());

pub async fn handle(hive: Hive, req: Request<Body>) -> Result<Response<Body>, Infallible> {
  let path = req.uri().path();
  let method = req.method();

  let result = if path == "/" {
    Ok(Response::new("Hello, world!".into()))
  } else if path == "/services" {
    match method {
      &Method::GET => unimplemented!("list"),
      &Method::POST => upload(&hive, req, None).await,
      _ => unimplemented!(),
    }
  } else if let Some(params) = SERVICES_WITH_NAME.gen_params(path) {
    match method {
      &Method::GET => unimplemented!("get_service"),
      &Method::PUT => upload(&hive, req, Some(params["name"].clone())).await,
      _ => unimplemented!(),
    }
  } else {
    unimplemented!("run_service")
  };

  match result {
    Ok(x) => Ok(x),
    Err(_error) => unimplemented!()
  }
}

async fn upload(
  hive: &Hive,
  mut req: Request<Body>,
  name: Option<Box<str>>,
) -> Result<Response<Body>, RawError> {
  let boundary = multer::parse_boundary(req.headers()["content-type"].to_str().unwrap())?;
  let constraints = Constraints::new()
    .allowed_fields(vec!["source"])
    .size_limit(SizeLimit::new().for_field("source", 1024u64.pow(3)));
  let mut multipart = Multipart::with_constraints(req.body_mut(), boundary, constraints);
  let source = multipart
    .next_field()
    .await
    .unwrap()
    .ok_or_else(|| -> hive_core::Error { unimplemented!("error no code") })?;
  let name = name
    .or_else(|| source.file_name().map(|x| slug::slugify(x).into()))
    .ok_or_else(|| -> hive_core::Error { unimplemented!("error no name") })?;
  let source = Source::new_single(source.bytes().await?.as_ref());

  hive.create_service(name, source).await?;

  Ok(Response::new("upload".into()))
}
