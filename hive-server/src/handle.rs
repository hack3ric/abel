use crate::error::method_not_allowed;
use crate::{json_response, Result};
use hive_core::{Hive, Service, Source};
use hyper::{Body, Method, Request, Response};
use log::error;
use multer::{Constraints, Multipart, SizeLimit};
use serde_json::json;
use std::convert::Infallible;

const GET: &Method = &Method::GET;
const POST: &Method = &Method::POST;
const PUT: &Method = &Method::PUT;
const DELETE: &Method = &Method::DELETE;

pub async fn handle(hive: Hive, req: Request<Body>) -> Result<Response<Body>, Infallible> {
  let method = req.method();
  let path = req.uri().path();
  let segments = path
    .split("/")
    .filter(|x| !x.is_empty())
    .collect::<Box<_>>();

  let result = match (method, &*segments) {
    (GET, []) => Ok(Response::new("\"Hello, world!\"".into())),

    (GET, ["services"]) => list(&hive).await,
    (POST, ["services"]) => upload(&hive, None, req).await,
    (_, ["services"]) => Err(method_not_allowed(&["GET", "POST"], method)),

    (GET, ["services", name]) => get_service(&hive, name).await,
    (PUT, ["services", name]) => upload(&hive, Some((*name).into()), req).await,
    (DELETE, ["services", _name]) => unimplemented!("DELETE /services/:name"),
    (_, ["services", _name]) => Err(method_not_allowed(&["GET", "PUT", "DELETE"], method)),

    (_, ["services", ..]) => Err((404, "hive path not found", json!({ "path": path })).into()),

    // TODO: solve self-referencing issue
    (_, [service_name, ..]) => {
      run_service(&hive, &service_name.to_string(), &path.to_string(), req).await
    }

    _ => Err((404, "hive path not found", json!({ "path": path })).into()),
  };

  Ok(result.unwrap_or_else(|error| {
    error!("{}", error);
    error.into_response(true)
  }))
}

async fn list(hive: &Hive) -> Result<Response<Body>> {
  let x = hive.list().await;
  let y = x.iter().map(Service::upgrade).collect::<Vec<_>>();
  Ok(json_response!(y))
}

async fn get_service(hive: &Hive, name: &str) -> Result<Response<Body>> {
  let service = hive.get_service(name).await?;
  Ok(json_response!(service.try_upgrade()?))
}

pub async fn upload(
  hive: &Hive,
  name: Option<Box<str>>,
  mut req: Request<Body>,
) -> Result<Response<Body>> {
  let name_provided = name.is_some();

  let content_type = req
    .headers()
    .get("Content-Type")
    .ok_or("no Content-Type given")?
    .to_str()
    .or(Err("Content-Type is not valid UTF-8"))?;

  let boundary = multer::parse_boundary(content_type)?;
  let constraints = Constraints::new()
    .allowed_fields(vec!["source"])
    .size_limit(SizeLimit::new().for_field("source", 1024u64.pow(3)));
  let mut multipart = Multipart::with_constraints(req.body_mut(), boundary, constraints);

  let field = multipart
    .next_field()
    .await?
    .ok_or("no source code uploaded")?;
  let name = name
    .or_else(|| {
      field.file_name().map(|mut x| {
        let len = x.len();
        if &x[len - 4..] == ".lua" {
          x = &x[..len - 4];
        }
        slug::slugify(x).into()
      })
    })
    .ok_or("no service name provided")?;

  if !name_provided && hive.get_service(&name).await.is_ok() {
    return Err((409, "service already exists", json!({ "name": name })).into());
  }

  let source = Source::new_single(field.bytes().await?.as_ref());
  let service = hive.create_service(name, source).await?;
  let service = service.upgrade();

  Ok(json_response!({ "new_service": &service }))
}

pub async fn run_service(
  hive: &Hive,
  service_name: &str,
  whole_path: &str,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let sub_path = "/".to_string() + whole_path[1..].split_once("/").unwrap_or(("", "")).1;
  let result = hive.run_service(service_name, sub_path, req).await?;
  Ok(result.into())
}
