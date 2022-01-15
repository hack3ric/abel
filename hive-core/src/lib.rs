mod error;
mod lua;
mod object_pool;
mod path;
mod service;
mod source;

pub use error::{Error, ErrorKind, Result};
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

  pub async fn get_service(&self, name: impl AsRef<str>) -> Result<Service> {
    let name = name.as_ref();
    self
      .service_pool
      .get_service(name)
      .await
      .ok_or_else(|| ErrorKind::ServiceNotFound(name.into()).into())
  }

  pub async fn run_service(&self, name: &str, path: String) -> Result<()> {
    let service = self.get_service(name).await?;
    self
      .sandbox_pool
      .scope(move |mut sandbox| async move {
        sandbox.run(service, &path).await?;
        Ok::<_, Error>(())
      })
      .await
      .unwrap()?;
    Ok(())
  }

  pub async fn list(&self) -> Vec<Service> {
    self.service_pool.list().await
  }
}
