use super::error::ErrorKind::Unauthorized;
use super::error::{method_not_allowed, ErrorAuthWrapper};
use super::types::{OwnedServiceWithStatus, ServiceWithStatus};
use super::upload::upload;
use super::{authenticate, json_response, Metadata, Result, ServerState};
use crate::server::types::ServiceStatus::{Running, Stopped};
use abel_core::ErrorKind::{ServiceDropped, ServiceNotFound};
use hyper::{Body, Method, Request, Response, StatusCode};
use log::{error, info};
use owo_colors::OwoColorize;
use serde::Deserialize;
use serde_json::json;
use std::borrow::Cow;
use std::convert::Infallible;
use std::sync::Arc;

pub(crate) async fn handle(
  state: Arc<ServerState>,
  req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
  const GET: &Method = &Method::GET;
  #[allow(unused)]
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

  let auth = authenticate(&state, &req);

  let result = match (method, &*segments) {
    (GET, []) => hello_world().await,

    // Service management API entry
    (_, ["services", ..]) => match (method, &segments[1..]) {
      _ if !auth => Err(Unauthorized.into()),
      (GET, []) => list(&state),
      (_, []) => Err(method_not_allowed(&["GET"], method)),

      (GET, [name]) => get(&state, name),
      (PUT, [name]) => upload(&state, (*name).into(), req).await,
      (PATCH, [name]) => start_stop(&state, name, req.uri().query().unwrap_or("")).await,
      (DELETE, [name]) => remove(&state, name).await,
      (_, [_name]) => Err(method_not_allowed(
        &["GET", "PUT", "PATCH", "DELETE"],
        method,
      )),

      (_, [..]) => Err((404, "path not found", json!({ "path": path })).into()),
    },

    // Service entry
    (_, [service_name, ..]) => {
      let sub_path = "/".to_string() + path[1..].split_once('/').unwrap_or(("", "")).1;
      let service_name: String = (*service_name).into();
      match state.abel.get_running_service(&service_name) {
        Ok(service) => {
          let result = state.abel.run_service(service, sub_path, req).await;
          match result {
            Ok(resp) => Ok(resp),
            // Hide `ServiceDropped` from normal users
            Err(error) if matches!(error.kind(), ServiceDropped) && !auth => {
              error!("{error}");
              Err(From::from(ServiceNotFound {
                name: service_name.into(),
              }))
            }
            Err(error) => Err(error.into()),
          }
        }
        Err(error) => Err(error.into()),
      }
    }

    _ => Err((404, "path not found", json!({ "path": path })).into()),
  };

  Ok(result.unwrap_or_else(|error| {
    let server_error = error.kind().status().is_server_error();
    let error = ErrorAuthWrapper::new(auth, error);
    if server_error {
      if let Some(uuid) = error.uuid() {
        error!("{error} {}", format!("({})", uuid).dimmed());
      } else {
        error!("{error}");
      }
    }
    error.into()
  }))
}

async fn hello_world() -> Result<Response<Body>> {
  json_response(StatusCode::OK, json!({ "msg": "Hello, world!" }))
}

fn list(state: &ServerState) -> Result<Response<Body>> {
  let services = state
    .abel
    .list_services()
    .map(OwnedServiceWithStatus::from)
    .collect::<Vec<_>>();
  json_response(StatusCode::OK, services)
}

fn get(state: &ServerState, name: &str) -> Result<Response<Body>> {
  let service = state.abel.get_service(name)?;
  json_response(
    StatusCode::OK,
    ServiceWithStatus::from_guard(&service.upgrade()),
  )
}

async fn start_stop(state: &ServerState, name: &str, query: &str) -> Result<Response<Body>> {
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
  let metadata_path = state
    .abel_path
    .join(format!("services/{name}/metadata.json"));

  match op {
    Operation::Start => {
      let service = state.abel.start_service(name).await?;
      Metadata::modify(&metadata_path, |m| m.started = true).await?;
      json_response(StatusCode::OK, ServiceWithStatus {
        status: Running,
        service: Cow::Borrowed(service.upgrade().info()),
      })
    }
    Operation::Stop => {
      let result = state.abel.stop_service(name).await;
      Metadata::modify(&metadata_path, |m| m.started = false).await?;
      result.map_err(From::from).and_then(|x| {
        json_response(StatusCode::OK, ServiceWithStatus {
          status: Stopped,
          service: Cow::Borrowed(x.info()),
        })
      })
    }
  }
}

async fn remove(state: &ServerState, service_name: &str) -> Result<Response<Body>> {
  let removed = state.abel.remove_service(service_name).await?;
  tokio::fs::remove_dir_all(state.abel_path.join("services").join(service_name)).await?;
  info!("Removed service '{}' ({})", removed.name(), removed.uuid());
  json_response(StatusCode::OK, removed.info())
}
