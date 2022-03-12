mod impls;

use crate::error::Result;
use crate::lua::{remove_service_local_storage, Sandbox};
use crate::source::Source;
use crate::task::Pool;
use crate::util::MyStr;
use crate::ErrorKind::*;
use crate::{Config, HiveState};
use dashmap::setref::multiple::RefMulti;
use dashmap::setref::one::Ref;
use dashmap::DashSet;
pub use impls::*;
use std::sync::Arc;
use uuid::Uuid;

pub struct ServicePool {
  services: DashSet<ServiceState>,
}

impl ServicePool {
  pub fn new() -> Self {
    Self {
      services: DashSet::new(),
    }
  }

  /// Creates a new service from source.
  pub async fn create(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: String,
    source: Source,
    config: Config,
  ) -> Result<RunningService> {
    if self.services.contains(MyStr::new(&*name)) {
      return Err(ServiceExists(name.into()).into());
    }

    let service_impl = sandbox_pool
      .scope(move |sandbox| async move {
        let Config {
          pkg_name,
          description,
          permissions,
        } = config;
        let permissions = Arc::new(permissions);
        let (paths, local_env, internal) = sandbox
          .pre_create_service(&name, source.clone(), permissions.clone())
          .await?;
        let service_impl = Arc::new(ServiceImpl {
          name: name.into_boxed_str(),
          pkg_name,
          description,
          paths,
          source,
          permissions,
          uuid: Uuid::new_v4(),
        });
        sandbox
          .finish_create_service(
            &service_impl.name,
            service_impl.downgrade(),
            local_env,
            internal,
          )
          .await?;
        Ok::<_, crate::Error>(service_impl)
      })
      .await?;
    let service = service_impl.downgrade();
    assert!(self.services.insert(ServiceState::Running(service_impl)));
    Ok(service)
  }

  pub async fn get_running(&self, name: &str) -> Option<RunningService> {
    let x = self.services.get::<MyStr>(name.into());
    if let Some(ServiceState::Running(x)) = x.as_deref() {
      Some(x.downgrade())
    } else {
      None
    }
  }

  pub async fn list(&self) -> (Vec<RunningService>, Vec<RefMulti<'_, ServiceState>>) {
    let mut running = Vec::new();
    let mut stopped = Vec::new();
    for service in self.services.iter() {
      match &*service {
        ServiceState::Running(x) => running.push(x.downgrade()),
        ServiceState::Stopped(_) => stopped.push(service),
      }
    }
    (running, stopped)
  }

  pub async fn stop(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: &str,
  ) -> Result<Ref<'_, ServiceState>> {
    if let Some(service) = self.services.remove(MyStr::new(name)) {
      if let ServiceState::Running(service) = service {
        let service2 = service.clone();
        sandbox_pool
          .scope(|sandbox| async move {
            sandbox.run_stop(service2.downgrade()).await?;
            Ok::<_, crate::Error>(())
          })
          .await?;
        let stopped = ServiceState::Running(service).into_impl();
        assert!(self.services.insert(ServiceState::Stopped(stopped)));
        Ok(self.services.get(MyStr::new(name)).unwrap())
      } else {
        assert!(self.services.insert(service));
        Err(ServiceStopped(name.into()).into())
      }
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }

  pub async fn start(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<RunningService> {
    if let Some(service) = self.services.remove(MyStr::new(name)) {
      if let ServiceState::Stopped(service) = service {
        let running = Arc::new(service);
        let service = running.clone();
        sandbox_pool
          .scope(move |sandbox| async move {
            sandbox.run_start(service.downgrade()).await?;
            Ok::<_, crate::Error>(())
          })
          .await?;
        assert!(self.services.insert(ServiceState::Running(running.clone())));
        Ok(running.downgrade())
      } else {
        assert!(self.services.insert(service));
        Err(ServiceRunning(name.into()).into())
      }
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }

  pub async fn remove(&self, state: &HiveState, name: &str) -> Result<ServiceImpl> {
    if let Some(old_service) = self.services.remove(MyStr::new(name)) {
      if let ServiceState::Stopped(x) = old_service {
        remove_service_local_storage(state, name).await?;
        Ok(x)
      } else {
        assert!(self.services.insert(old_service));
        Err(ServiceRunning(name.into()).into())
      }
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }
}
