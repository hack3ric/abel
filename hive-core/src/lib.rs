#![feature(backtrace)]
#![allow(unused)]
#![warn(unused_imports)]

mod error;
mod lua;
mod object_pool;
mod service;
mod source;

pub use error::{Error, HiveResult};
pub use service::{Service, ServiceGuard};

use lua::Sandbox;
use object_pool::Pool;
use service::ServicePool;
use source::Source;

pub struct Hive {
  sandbox_pool: Pool<Sandbox>,
  service_pool: ServicePool,
}

impl Hive {
  pub fn new() -> HiveResult<Self> {
    Ok(Self {
      sandbox_pool: Pool::with_capacity(8, Sandbox::new)?,
      service_pool: ServicePool::new(),
    })
  }

  pub async fn create_service(
    &self,
    name: impl Into<String>,
    source: Source,
  ) -> HiveResult<Service> {
    self
      .service_pool
      .create_service(&self.sandbox_pool, name, source)
      .await
  }
}
