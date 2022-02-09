use crate::error::method_not_allowed;
use crate::util::{json_response, SingleMainLua};
use crate::{MainState, Result};
use hive_core::{ErrorKind, Service, Source};
use hyper::{Body, Method, Request, Response, StatusCode};
use log::error;
use multer::{Constraints, Multipart, SizeLimit};
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::fs;

const GET: &Method = &Method::GET;
const POST: &Method = &Method::POST;
const PUT: &Method = &Method::PUT;
const DELETE: &Method = &Method::DELETE;

pub(crate) async fn handle(
  state: Arc<MainState>,
  req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
  let method = req.method();
  let path = req.uri().path();
  let segments = path
    .split("/")
    .filter(|x| !x.is_empty())
    .collect::<Box<_>>();

  let result = match (method, &*segments) {
    (GET, []) => Ok(Response::new("\"Hello, world!\"".into())),

    (GET, ["services"]) => list(&state).await,
    (POST, ["services"]) => upload(&state, None, req).await,
    (_, ["services"]) => Err(method_not_allowed(&["GET", "POST"], method)),

    (GET, ["services", name]) => get(&state, name).await,
    (PUT, ["services", name]) => upload(&state, Some((*name).into()), req).await,
    (DELETE, ["services", name]) => remove(&state, name).await,
    (_, ["services", _name]) => Err(method_not_allowed(&["GET", "PUT", "DELETE"], method)),

    (_, ["services", ..]) => Err((404, "hive path not found", json!({ "path": path })).into()),

    // TODO: solve self-referencing issue
    (_, [service_name, ..]) => run(&state, &service_name.to_string(), &path.to_string(), req).await,

    _ => Err((404, "hive path not found", json!({ "path": path })).into()),
  };

  Ok(result.unwrap_or_else(|error| {
    error!("{}", error);
    error.into_response(true)
  }))
}

async fn list(state: &MainState) -> Result<Response<Body>> {
  let x = state.hive.list_services().await;
  let y = x.iter().map(Service::upgrade).collect::<Vec<_>>();
  Ok(json_response(StatusCode::OK, y))
}

async fn get(state: &MainState, name: &str) -> Result<Response<Body>> {
  let service = state.hive.get_service(name).await?;
  Ok(json_response(StatusCode::OK, service.try_upgrade()?))
}

async fn upload(
  state: &MainState,
  name: Option<String>,
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

  if !name_provided && state.hive.get_service(&name).await.is_ok() {
    return Err((409, "service already exists", json!({ "name": name })).into());
  }

  let replaced = match state.hive.remove_service(&name).await {
    Ok(replaced) => Some(replaced),
    Err(error) if matches!(error.kind(), ErrorKind::ServiceNotFound(_)) => None,
    Err(error) => return Err(error.into()),
  };

  // TODO: returns `replaced` when this part fails
  let service = async {
    let source_path = state.config_path.join(format!("services/{}", name));
    if source_path.exists() {
      fs::remove_dir_all(&source_path).await?;
    }
    fs::create_dir(&source_path).await?;

    let source_bytes = field.bytes().await?;
    fs::write(source_path.join("main.lua"), &source_bytes).await?;
    let vfs = SingleMainLua::from_slice(source_bytes);
    let source = Source::new(vfs);

    let service = state.hive.create_service(name, source).await?;
    Ok::<_, crate::error::Error>(service)
  }
  .await?;
  let service = service.upgrade();

  let response = if let Some(replaced) = replaced {
    json_response(
      StatusCode::OK,
      json!({
        "new_service": service,
        "replaced_service": replaced
      }),
    )
  } else {
    json_response(StatusCode::OK, json!({ "new_service": service }))
  };
  Ok(response)
}

async fn run(
  state: &MainState,
  service_name: &str,
  whole_path: &str,
  req: Request<Body>,
) -> Result<Response<Body>> {
  let sub_path = "/".to_string() + whole_path[1..].split_once("/").unwrap_or(("", "")).1;
  let result = state.hive.run_service(service_name, sub_path, req).await?;
  Ok(result.into())
}

async fn remove(state: &MainState, service_name: &str) -> Result<Response<Body>> {
  let removed = state.hive.remove_service(service_name).await?;
  Ok(json_response(
    StatusCode::OK,
    json!({ "removed_service": removed }),
  ))
}
