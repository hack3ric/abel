mod impls;

use crate::lua::{remove_service_local_storage, Sandbox};
use crate::source::Source;
use crate::task::Pool;
use crate::ErrorKind::*;
use crate::{Config, HiveState, Result};
use dashmap::DashMap;
pub use impls::*;
use log::{error, warn};
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use smallstr::SmallString;
use std::sync::Arc;
use uuid::Uuid;

pub type ServiceName = SmallString<[u8; 16]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceLoadMode {
  Load,
  ColdUpdate,
  HotUpdate,
}

#[derive(Default)]
pub struct ServicePool {
  services: DashMap<ServiceName, ServiceState>,
}

impl ServicePool {
  pub fn new() -> Self {
    Default::default()
  }

  pub async fn create(
    &self,
    mode: ServiceLoadMode,
    sandbox_pool: &Pool<Sandbox>,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: Source,
    config: Config,
  ) -> Result<(Service<'_>, Option<ServiceImpl>)> {
    let hot_update = self.services.contains_key(&*name) && mode == ServiceLoadMode::HotUpdate;

    let name2 = name.clone();
    let service_state = sandbox_pool
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
        let service_impl = ServiceImpl {
          name: name2.clone(),
          pkg_name,
          description,
          paths,
          source,
          permissions,
          uuid: uuid.unwrap_or_else(Uuid::new_v4),
        };
        let state = if mode == ServiceLoadMode::Load {
          sandbox.remove_registry(local_env)?;
          sandbox.remove_registry(internal)?;
          ServiceState::Stopped(service_impl)
        } else {
          let service_impl = Arc::new(service_impl);
          let result = sandbox
            .finish_create_service(
              &service_impl.name,
              service_impl.downgrade(),
              local_env,
              internal,
              hot_update,
            )
            .await;
          match result {
            Ok(()) => ServiceState::Running(service_impl),
            // TODO: return the error
            Err(err) => {
              error!("failed running `hive.start` in {name2}: {err}");
              let service_impl = Arc::try_unwrap(service_impl).unwrap_or_else(|x| (*x).clone());
              sandbox.expire_registry_values();
              ServiceState::Stopped(service_impl)
            }
          }
        };
        Ok::<_, crate::Error>(state)
      })
      .await?;

    match service_state {
      ServiceState::Running(service_impl) => {
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
          .insert(name, ServiceState::Running(service_impl))
          .is_none());
        Ok((Service::Running(service), replaced))
      }
      ServiceState::Stopped(_) => {
        assert!(self.services.insert(name.clone(), service_state).is_none());
        let service = self.services.get(&*name).unwrap();
        Ok((Service::Stopped(StoppedService::from_ref(service)), None))
      }
    }
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

  pub async fn stop_all(&self, sandbox_pool: &Pool<Sandbox>) {
    for mut service in self.services.iter_mut() {
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
        if let Err(error) = result {
          warn!(
            "Lua error when stopping service '{}': {error}",
            service.key()
          )
        }
      }
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
