mod upload;

use crate::error::method_not_allowed;
use crate::util::json_response;
use crate::{MainState, Result};
use hive_core::Service;
use hyper::{Body, Method, Request, Response, StatusCode};
use log::error;
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use upload::upload;

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
    (_, [service_name, ..]) => {
      let sub_path = "/".to_string() + path[1..].split_once("/").unwrap_or(("", "")).1;
      (state.hive)
        .run_service(&service_name.to_string(), sub_path, req)
        .await
        .map(From::from)
        .map_err(From::from)
    }

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

async fn remove(state: &MainState, service_name: &str) -> Result<Response<Body>> {
  let removed = state.hive.remove_service(service_name).await?;
  Ok(json_response(
    StatusCode::OK,
    json!({ "removed_service": removed }),
  ))
}
