mod impls;

pub use impls::*;
use crate::error::Result;
use crate::lua::Sandbox;
use crate::source::Source;
use crate::task::Pool;
use crate::util::MyStr;
use crate::Config;
use crate::ErrorKind::*;
use dashmap::DashSet;
use std::sync::Arc;
use uuid::Uuid;
use crate::HiveState;
use crate::lua::remove_service_local_storage;

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
  ) -> Result<LiveService> {
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
    assert!(self.services.insert(ServiceState::Live(service_impl)));
    Ok(service)
  }

  pub async fn get_live(&self, name: &str) -> Option<LiveService> {
    let x = self
      .services
      .get::<MyStr>(name.into());
    if let Some(ServiceState::Live(x)) = x.as_deref() {
      Some(x.downgrade())
    } else {
      None
    }
  }

  pub async fn list(&self) -> Vec<LiveService> {
    // self.services.iter().map(|x| x.downgrade()).collect()
    todo!()
  }

  pub async fn stop(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<()> {
    if let Some(service) = self.services.remove(MyStr::new(name)) {
      if let ServiceState::Live(service) = service {
        let service2 = service.clone();
        sandbox_pool
          .scope(|sandbox| async move {
            sandbox.run_stop(service2.downgrade()).await?;
            Ok::<_, crate::Error>(())
          })
          .await?;
        let stopped = Arc::try_unwrap(service).unwrap_or_else(|arc| arc.as_ref().clone());
        assert!(self.services.insert(ServiceState::Stopped(stopped)));
        Ok(())
      } else {
        assert!(self.services.insert(service));
        Err(ServiceStopped(name.into()).into())
      }
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }

  pub async fn start(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<()> {
    if let Some(service) = self.services.remove(MyStr::new(name)) {
      if let ServiceState::Stopped(service) = service {
        let live = Arc::new(service);
        let service = live.clone();
        sandbox_pool
          .scope(move |sandbox| async move {
            sandbox.run_start(service.downgrade()).await?;
            Ok::<_, crate::Error>(())
          })
          .await?;
        assert!(self.services.insert(ServiceState::Live(live)));
        Ok(())
      } else {
        assert!(self.services.insert(service));
        Err(ServiceLive(name.into()).into())
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
        Err(ServiceLive(name.into()).into())
      }
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }
}
