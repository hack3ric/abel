mod upload;

use crate::error::method_not_allowed;
use crate::util::json_response;
use crate::{MainState, Result};
use hive_core::RunningService;
use hyper::{Body, Method, Request, Response, StatusCode};
use log::error;
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::ops::Deref;
use std::sync::Arc;
use upload::upload;

pub(crate) async fn handle(
  state: Arc<MainState>,
  req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
  const GET: &Method = &Method::GET;
  const POST: &Method = &Method::POST;
  const PUT: &Method = &Method::PUT;
  const PATCH: &Method = &Method::PATCH;
  const DELETE: &Method = &Method::DELETE;

  let method = req.method();
  let path = req.uri().path();
  let segments = path
    .split('/')
    .filter(|x| !x.is_empty())
    .collect::<Box<_>>();

  let result = match (method, &*segments) {
    (GET, []) => hello_world().await,

    (GET, ["services"]) => list(&state).await,
    (POST, ["services"]) => upload(&state, None, req).await,
    (_, ["services"]) => Err(method_not_allowed(&["GET", "POST"], method)),

    (GET, ["services", name]) => get(&state, name).await,
    (PUT, ["services", name]) => upload(&state, Some((*name).into()), req).await,
    (PATCH, ["services", name]) => start_stop(&state, name, req.uri().query().unwrap_or("")).await,
    (DELETE, ["services", name]) => remove(&state, name).await,
    (_, ["services", _name]) => Err(method_not_allowed(
      &["GET", "PUT", "PATCH", "DELETE"],
      method,
    )),

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

async fn hello_world() -> Result<Response<Body>> {
  json_response(StatusCode::OK, json!({ "msg": "Hello, world!" }))
}

async fn list(state: &MainState) -> Result<Response<Body>> {
  let (running, stopped) = state.hive.list_services().await;
  let running = running.iter().map(RunningService::upgrade).collect::<Vec<_>>();
  let stopped = stopped.iter().map(Deref::deref).collect::<Vec<_>>();
  json_response(StatusCode::OK, json!({ "running": running, "stopped": stopped }))
}

async fn get(state: &MainState, name: &str) -> Result<Response<Body>> {
  let service = state.hive.get_service(name).await?;
  json_response(StatusCode::OK, service.try_upgrade()?)
}

async fn start_stop(state: &MainState, name: &str, query: &str) -> Result<Response<Body>> {
  #[derive(Deserialize)]
  struct Query {
    op: Operation,
  }

  #[derive(Deserialize)]
  enum Operation {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "stop")]
    Stop,
  }

  let Query { op } = serde_qs::from_str(query)?;

  match op {
    Operation::Start => {
      let service = state.hive.start_service(name).await?;
      json_response(StatusCode::OK, json!({ "started": service.upgrade() }))
    }
    Operation::Stop => {
      let service = state.hive.stop_service(name).await?;
      json_response(StatusCode::OK, json!({ "stopped": &*service }))
    }
  }
}

async fn remove(state: &MainState, service_name: &str) -> Result<Response<Body>> {
  let removed = state.hive.remove_service(service_name).await?;
  tokio::fs::remove_dir_all(state.config_path.join("services").join(service_name)).await?;
  json_response(StatusCode::OK, json!({ "removed_service": removed }))
}
