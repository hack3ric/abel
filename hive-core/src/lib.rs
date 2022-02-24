pub mod permission;

mod error;
mod lua;
mod path;
mod service;
mod source;
mod task;

pub use error::{Error, ErrorKind, Result};
pub use lua::request::Request;
pub use lua::response::Response;
pub use mlua::Error as LuaError;
pub use service::{Service, ServiceGuard};
pub use source::Source;

use hyper::Body;
use lua::Sandbox;
use permission::PermissionSet;
use service::ServicePool;
use task::Pool;

pub struct Hive {
  sandbox_pool: Pool<Sandbox>,
  service_pool: ServicePool,
}

pub struct HiveOptions {
  pub sandbox_pool_size: usize,
}

impl Hive {
  pub fn new(options: HiveOptions) -> Result<Self> {
    Ok(Self {
      sandbox_pool: Pool::new("hive-worker", options.sandbox_pool_size, Sandbox::new)?,
      service_pool: ServicePool::new(),
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
    let name = name.as_ref();
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
