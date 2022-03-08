pub mod permission;

mod error;
mod lua;
mod path;
mod service;
mod source;
mod task;

pub use error::{Error, ErrorKind, Result};
pub use lua::http::{Request, Response};
pub use mlua::Error as LuaError;
pub use service::{Service, ServiceGuard};
pub use source::Source;

use hyper::Body;
use lua::Sandbox;
use permission::PermissionSet;
use service::ServicePool;
use std::path::PathBuf;
use std::sync::Arc;
use task::Pool;

pub struct Hive {
  sandbox_pool: Pool<Sandbox>,
  service_pool: ServicePool,
  #[allow(unused)]
  state: Arc<HiveState>,
}

#[derive(Debug)]
pub struct HiveState {
  pub local_storage_path: PathBuf,
}

pub struct HiveOptions {
  pub sandbox_pool_size: usize,
  pub local_storage_path: PathBuf,
}

impl Hive {
  pub fn new(options: HiveOptions) -> Result<Self> {
    let state = Arc::new(HiveState {
      local_storage_path: options.local_storage_path,
    });
    let state2 = state.clone();
    Ok(Self {
      sandbox_pool: Pool::new("hive-worker", options.sandbox_pool_size, move || {
        Sandbox::new(state2.clone())
      })?,
      service_pool: ServicePool::new(),
      state,
    })
  }

  pub async fn create_service(
    &self,
    name: String,
    source: Source,
    permissions: PermissionSet,
  ) -> Result<Service> {
    self
      .service_pool
      .create(&self.sandbox_pool, name, source, permissions)
      .await
  }

  pub async fn get_service(&self, name: &str) -> Result<Service> {
    self
      .service_pool
      .get(name)
      .await
      .ok_or_else(|| ErrorKind::ServiceNotFound(name.into()).into())
  }

  pub async fn run_service(
    &self,
    name: &str,
    path: String,
    req: hyper::Request<Body>,
  ) -> Result<Response> {
    let service = self.get_service(name).await?;
    self
      .sandbox_pool
      .scope(move |sandbox| async move { sandbox.run(service, &path, req).await })
      .await
  }

  pub async fn list_services(&self) -> Vec<Service> {
    self.service_pool.list().await
  }

  pub async fn remove_service(&self, name: &str) -> Result<ServiceGuard<'_>> {
    self.service_pool.remove(&self.sandbox_pool, name).await
  }
}
