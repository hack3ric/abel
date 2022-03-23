mod impls;

use crate::lua::{remove_service_local_storage, Sandbox};
use crate::source::Source;
use crate::task::Pool;
use crate::ErrorKind::*;
use crate::{Config, HiveState, Result};
use dashmap::DashMap;
pub use impls::*;
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
pub struct ServicePool {
  // services: DashSet<ServiceState>,
  services: DashMap<Box<str>, ServiceState>,
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
    uuid: Option<Uuid>,
    source: Source,
    config: Config,
    hot_update: bool,
  ) -> Result<(RunningService, Option<ServiceImpl>)> {
    let hot_update = self.services.contains_key(&*name) && hot_update;

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
          uuid: uuid.unwrap_or_else(Uuid::new_v4),
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
        .get(&*name)
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
      .remove(&*name)
      .map(|(_name, service)| service.into_impl());
    assert!(self
      .services
      .insert(name.into(), ServiceState::Running(service_impl))
      .is_none());
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
    if self.services.contains_key(&*name) {
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
    assert!(self
      .services
      .insert(name.clone().into(), service_state)
      .is_none());
    let service = self.services.get(&*name).unwrap();
    Ok(StoppedService::from_ref(service))
  }

  pub fn get(&self, name: &str) -> Option<Service<'_>> {
    self.services.get(name).map(|x| match x.value() {
      ServiceState::Running(x) => Service::Running(x.downgrade()),
      ServiceState::Stopped(_) => Service::Stopped(StoppedService::from_ref(x)),
    })
  }

  pub fn get_running(&self, name: &str) -> Option<RunningService> {
    let x = self.services.get(name);
    if let Some(ServiceState::Running(x)) = x.as_deref() {
      Some(x.downgrade())
    } else {
      None
    }
  }

  pub fn list(&self) -> impl Iterator<Item = Service<'_>> {
    self.services.iter().map(|x| match x.value() {
      ServiceState::Running(x) => Service::Running(x.downgrade()),
      ServiceState::Stopped(_) => Service::Stopped(StoppedService::from_ref_multi(x)),
    })
  }

  pub async fn stop(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<StoppedService<'_>> {
    if let Some(mut service) = self.services.get_mut(name) {
      let state = service.value_mut();
      if let ServiceState::Running(service2) = state {
        let x = service2.downgrade();
        let result = sandbox_pool
          .scope(|sandbox| async move {
            sandbox.run_stop(x).await?;
            Ok::<_, crate::Error>(())
          })
          .await;
        replace_with_or_abort(state, |x| ServiceState::Stopped(x.into_impl()));
        result.map(|_| StoppedService::from_ref(service.downgrade()))
      } else {
        Err(ServiceStopped { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
    }
  }

  pub async fn start(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<RunningService> {
    if let Some(mut service) = self.services.get_mut(name) {
      if let state @ ServiceState::Stopped(_) = service.value_mut() {
        let running = replace_with_or_abort_and_return(state, |x| {
          if let ServiceState::Stopped(s) = x {
            let s = Arc::new(s);
            (s.downgrade(), ServiceState::Running(s))
          } else {
            unreachable!()
          }
        });
        let running2 = running.clone();
        let result = sandbox_pool
          .scope(move |sandbox| async move {
            sandbox.run_start(running2).await?;
            Ok::<_, crate::Error>(())
          })
          .await;
        match result {
          Ok(_) => Ok(running),
          Err(error) => {
            replace_with_or_abort(state, |x| ServiceState::Stopped(x.into_impl()));
            Err(error)
          }
        }
      } else {
        Err(ServiceRunning { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
    }
  }

  pub async fn remove(&self, state: &HiveState, name: &str) -> Result<ServiceImpl> {
    if let Some((name2, old_service)) = self.services.remove(name) {
      if let ServiceState::Stopped(x) = old_service {
        remove_service_local_storage(state, name).await?;
        Ok(x)
      } else {
        assert!(self.services.insert(name2, old_service).is_none());
        Err(ServiceRunning { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
    }
  }
}
