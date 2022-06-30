mod create;
mod impls;

pub use create::ErrorPayload;
pub use impls::*;

use crate::lua::Runtime;
use crate::task::RuntimePool;
use crate::ErrorKind::*;
use crate::{HiveState, Result};
use dashmap::DashMap;
use log::warn;
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use smallstr::SmallString;
use std::sync::Arc;

pub type ServiceName = SmallString<[u8; 16]>;
type Services = DashMap<ServiceName, ServiceState>;

#[derive(Default)]
pub struct ServicePool {
  services: Arc<Services>,
}

impl ServicePool {
  pub fn new() -> Self {
    Default::default()
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

  pub async fn stop(&self, rt_pool: &RuntimePool, name: &str) -> Result<StoppedService<'_>> {
    if let Some(mut service) = self.services.get_mut(name) {
      let state = service.value_mut();
      if let ServiceState::Running(service2) = state {
        let x = service2.downgrade();
        let result = rt_pool
          .scope(|rt| async move {
            rt.run_stop(x).await?;
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

  async fn scope_stop(services: Arc<Services>, rt: &Runtime, name: &str) -> Result<()> {
    if let Some(mut service) = services.get_mut(name) {
      let state = service.value_mut();
      if let ServiceState::Running(service2) = state {
        let x = service2.downgrade();
        let result = rt.run_stop(x).await;
        replace_with_or_abort(state, |x| ServiceState::Stopped(x.into_impl()));
        result
      } else {
        Err(ServiceStopped { name: name.into() }.into())
      }
    } else {
      Err(ServiceNotFound { name: name.into() }.into())
    }
  }

  pub async fn stop_all(&self, rt_pool: &RuntimePool) {
    for mut service in self.services.iter_mut() {
      let state = service.value_mut();
      if let ServiceState::Running(service2) = state {
        let x = service2.downgrade();
        let result = rt_pool
          .scope(|rt| async move {
            rt.run_stop(x).await?;
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

  pub async fn start(&self, rt_pool: &RuntimePool, name: &str) -> Result<RunningService> {
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
        let result = rt_pool
          .scope(move |rt| async move {
            rt.run_start(running2).await?;
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
        let local_storage_path = state.local_storage_path.join(name);
        tokio::fs::remove_dir_all(local_storage_path).await?;
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
