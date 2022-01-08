pub mod path;

mod error;
mod lua;
mod object_pool;
mod service;
mod source;

pub use error::{Error, Result};
pub use service::{Service, ServiceGuard};
pub use source::Source;

use lua::Sandbox;
use object_pool::Pool;
use service::ServicePool;

#[derive(Clone)]
pub struct Hive {
  sandbox_pool: Pool<Sandbox>,
  service_pool: ServicePool,
}

impl Hive {
  pub fn new() -> Result<Self> {
    Ok(Self {
      sandbox_pool: Pool::with_capacity(8, Sandbox::new)?,
      service_pool: ServicePool::new(),
    })
  }

  pub async fn create_service(&self, name: impl Into<String>, source: Source) -> Result<Service> {
    self
      .service_pool
      .create_service(&self.sandbox_pool, name, source)
      .await
  }

  pub async fn get_service(&self, name: impl AsRef<str>) -> Option<Service> {
    self.service_pool.get_service(name).await
  }
}
