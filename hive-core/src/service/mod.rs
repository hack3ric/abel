mod impls;

use crate::lua::{remove_service_local_storage, Sandbox};
use crate::source::Source;
use crate::task::Pool;
use crate::util::MyStr;
use crate::ErrorKind::*;
use crate::{Config, HiveState, Result};
use dashmap::DashSet;
pub use impls::*;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
pub struct ServicePool {
  services: DashSet<ServiceState>,
}

impl ServicePool {
  pub fn new() -> Self {
    Default::default()
  }

  /// Creates a new service, or updates an existing service from source.
  pub async fn create(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: String,
    source: Source,
    config: Config,
    hot_update: bool,
  ) -> Result<(RunningService, Option<ServiceImpl>)> {
    let hot_update = self.services.contains(MyStr::new(&name)) && hot_update;

    let name2 = name.clone();
    let service_impl = sandbox_pool
      .scope(move |sandbox| async move {
        let Config {
          pkg_name,
          description,
          permissions,
        } = config;
        let permissions = Arc::new(permissions);
        let (paths, local_env, internal) = sandbox
          .pre_create_service(&name2, source.clone(), permissions.clone())
          .await?;
        let service_impl = Arc::new(ServiceImpl {
          name: name2.into_boxed_str(),
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
            hot_update,
          )
          .await?;
        Ok::<_, crate::Error>(service_impl)
      })
      .await?;
    let service = service_impl.downgrade();
    if !hot_update
      && self
        .services
        .get(MyStr::new(&name))
        .map(|x| matches!(&*x, ServiceState::Running(_)))
        .unwrap_or(false)
    {
      match self.stop(sandbox_pool, &name).await {
        Ok(_) => {}
        Err(x) if matches!(x.kind(), ServiceStopped { .. } | ServiceNotFound { .. }) => {}
        Err(error) => return Err(error),
      }
    }
    let replaced = (self.services)
      .remove(MyStr::new(&name))
      .map(ServiceState::into_impl);
    assert!(self.services.insert(ServiceState::Running(service_impl)));
    Ok((service, replaced))
  }

  pub async fn load(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: String,
    uuid: Uuid,
    source: Source,
    config: Config,
  ) -> Result<StoppedService<'_>> {
    if self.services.contains(MyStr::new(&name)) {
      return Err(ServiceExists { name: name.into() }.into());
    }

    let name2 = name.clone();
    let service_impl = sandbox_pool
      .scope(move |sandbox| async move {
        let Config {
          pkg_name,
          description,
          permissions,
        } = config;
        let permissions = Arc::new(permissions);
        let (paths, local_env, internal) = sandbox
          .pre_create_service(&name2, source.clone(), permissions.clone())
          .await?;
        sandbox.remove_registry(local_env)?;
        sandbox.remove_registry(internal)?;
        let service_impl = ServiceImpl {
          name: name2.into_boxed_str(),
          pkg_name,
          description,
          paths,
          source,
          permissions,
          uuid,
        };
        Ok::<_, crate::Error>(service_impl)
      })
      .await?;

    let service_state = ServiceState::Stopped(service_impl);
    assert!(self.services.insert(service_state));
    let service = self.services.get(MyStr::new(&name)).unwrap();
    Ok(StoppedService::from_ref(service))
  }

  pub fn get(&self, name: &str) -> Option<Service<'_>> {
    self.services.get(MyStr::new(name)).map(|x| match x.key() {
      ServiceState::Running(x) => Service::Running(x.downgrade()),
      ServiceState::Stopped(_) => Service::Stopped(StoppedService::from_ref(x)),
    })
  }

  pub fn get_running(&self, name: &str) -> Option<RunningService> {
    let x = self.services.get::<MyStr>(name.into());
    if let Some(ServiceState::Running(x)) = x.as_deref() {
      Some(x.downgrade())
    } else {
      None
    }
  }

  pub fn list(&self) -> impl Iterator<Item = Service<'_>> {
    self.services.iter().map(|x| match x.key() {
      ServiceState::Running(x) => Service::Running(x.downgrade()),
      ServiceState::Stopped(_) => Service::Stopped(StoppedService::from_ref_multi(x)),
    })
  }

  pub async fn stop(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<StoppedService<'_>> {
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
        let service = self.services.get(MyStr::new(name)).unwrap();
        Ok(StoppedService::from_ref(service))
      } else {
        assert!(self.services.insert(service));
        Err(ServiceStopped { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
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
        Err(ServiceRunning { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
    }
  }

  pub async fn remove(&self, state: &HiveState, name: &str) -> Result<ServiceImpl> {
    if let Some(old_service) = self.services.remove(MyStr::new(name)) {
      if let ServiceState::Stopped(x) = old_service {
        remove_service_local_storage(state, name).await?;
        Ok(x)
      } else {
        assert!(self.services.insert(old_service));
        Err(ServiceRunning { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
    }
  }
}
