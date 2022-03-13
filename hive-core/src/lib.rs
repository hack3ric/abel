pub mod permission;

mod config;
mod error;
mod lua;
mod path;
mod service;
mod source;
mod task;
mod util;

pub use config::Config;
pub use error::{Error, ErrorKind, Result};
pub use lua::http::{LuaRequest, LuaResponse};
pub use mlua::Error as LuaError;
pub use service::{RunningService, RunningServiceGuard, ServiceImpl};
pub use source::Source;

use dashmap::setref::multiple::RefMulti;
use dashmap::setref::one::Ref;
use hyper::{Body, Request};
use lua::Sandbox;
use service::{ServicePool, ServiceState};
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
      sandbox_pool: Pool::new(
        "hive-worker".to_string(),
        options.sandbox_pool_size,
        move || Sandbox::new(state2.clone()),
      )?,
      service_pool: ServicePool::new(),
      state,
    })
  }

  pub async fn create_service(
    &self,
    name: String,
    source: Source,
    config: Config,
  ) -> Result<RunningService> {
    (self.service_pool)
      .create(&self.sandbox_pool, name, source, config)
      .await
  }

  pub async fn get_service(&self, name: &str) -> Result<RunningService> {
    (self.service_pool)
      .get_running(name)
      .await
      .ok_or_else(|| ErrorKind::ServiceNotFound { name: name.into() }.into())
  }

  pub async fn run_service(
    &self,
    name: &str,
    path: String,
    req: Request<Body>,
  ) -> Result<LuaResponse> {
    let service = self.get_service(name).await?;
    self
      .sandbox_pool
      .scope(move |sandbox| async move { sandbox.run(service, &path, req).await })
      .await
  }

  pub async fn list_services(&self) -> (Vec<RunningService>, Vec<RefMulti<'_, ServiceState>>) {
    self.service_pool.list().await
  }

  pub async fn stop_service(&self, name: &str) -> Result<Ref<'_, ServiceState>> {
    self.service_pool.stop(&self.sandbox_pool, name).await
  }

  pub async fn start_service(&self, name: &str) -> Result<RunningService> {
    self.service_pool.start(&self.sandbox_pool, name).await
  }

  pub async fn remove_service(&self, name: &str) -> Result<ServiceImpl> {
    self.service_pool.remove(&self.state, name).await
  }
}
