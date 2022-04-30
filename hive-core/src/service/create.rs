use super::{
  RunningService, Service, ServiceImpl, ServiceName, ServicePool, ServiceState, StoppedService,
};
use crate::lua::Sandbox;
use crate::source::DirSource;
use crate::task::Pool;
use crate::ErrorKind::{self, ServiceNotFound, ServiceStopped};
use crate::{Config, Error, Result};
use mlua::RegistryKey;
use std::sync::Arc;
use uuid::Uuid;

/// Contains non-critical errors when loading, creating or updating services.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ErrorPayload {
  pub stop: Option<Error>,
  pub start: Option<Error>,
}

impl ErrorPayload {
  pub fn empty() -> Self {
    Default::default()
  }

  pub fn is_empty(&self) -> bool {
    self.stop.is_none() && self.start.is_none()
  }
}

async fn prepare_service(
  sandbox: &Sandbox,
  name: ServiceName,
  uuid: Option<Uuid>,
  source: DirSource,
  config: Config,
) -> Result<(ServiceImpl, RegistryKey, RegistryKey)> {
  let Config {
    pkg_name,
    description,
    permissions,
  } = config;
  let permissions = Arc::new(permissions);
  let (paths, local_env, internal) = sandbox
    .prepare_service(&name, source.clone(), permissions.clone())
    .await?;
  let service_impl = ServiceImpl {
    name,
    pkg_name,
    description,
    paths,
    source,
    permissions,
    uuid: uuid.unwrap_or_else(Uuid::new_v4),
  };
  Ok((service_impl, local_env, internal))
}

impl ServicePool {
  pub async fn load(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: DirSource,
    config: Config,
  ) -> Result<(StoppedService<'_>, Option<ServiceImpl>, ErrorPayload)> {
    let services = self.services.clone();
    let name2 = name.clone();
    let (service_impl, error_payload) = sandbox_pool
      .scope(move |sandbox| async move {
        let mut error_payload = ErrorPayload::empty();

        let (service_impl, local_env, internal) =
          prepare_service(&sandbox, name2.clone(), uuid, source, config).await?;
        sandbox.remove_registry(local_env)?;
        sandbox.remove_registry(internal)?;

        match Self::scope_stop(services, &sandbox, &*name2).await {
          Ok(_) => {}
          Err(error) if matches!(error.kind(), ServiceStopped { .. } | ServiceNotFound { .. }) => {}
          Err(error) => error_payload.stop = Some(error),
        }

        Ok::<_, crate::Error>((service_impl, error_payload))
      })
      .await?;

    let replaced = (self.services)
      .remove(&*name)
      .map(|(_name, service)| service.into_impl());
    assert!(self
      .services
      .insert(name.clone(), ServiceState::Stopped(service_impl))
      .is_none());
    let service = self.services.get(&*name).unwrap();
    Ok((StoppedService::from_ref(service), replaced, error_payload))
  }

  pub async fn cold_update_or_create(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: DirSource,
    config: Config,
  ) -> Result<(Service<'_>, Option<ServiceImpl>, ErrorPayload)> {
    let services = self.services.clone();
    let name2 = name.clone();
    let (service_state, error_payload) = sandbox_pool
      .scope(move |sandbox| async move {
        let mut error_payload = ErrorPayload::default();

        let (service_impl, local_env, internal) =
          prepare_service(&sandbox, name2.clone(), uuid, source, config).await?;

        match Self::scope_stop(services, &sandbox, &*name2).await {
          Ok(_) => {}
          Err(error) if matches!(error.kind(), ServiceStopped { .. } | ServiceNotFound { .. }) => {}
          Err(error) => error_payload.stop = Some(error),
        }

        let service_impl = Arc::new(service_impl);
        let result = sandbox
          .create_service(
            service_impl.name(),
            service_impl.downgrade(),
            local_env,
            internal,
            false,
          )
          .await;
        let state = ServiceState::Running(service_impl);
        let state = match result {
          Ok(()) => state,
          Err(err) => {
            error_payload.start = Some(err);
            let service_impl = state.into_impl();
            sandbox.expire_registry_values();
            ServiceState::Stopped(service_impl)
          }
        };

        Ok::<_, crate::Error>((state, error_payload))
      })
      .await?;

    match service_state {
      ServiceState::Running(service_impl) => {
        let service = service_impl.downgrade();
        let replaced = (self.services)
          .remove(&*name)
          .map(|(_name, service)| service.into_impl());
        assert!(self
          .services
          .insert(name, ServiceState::Running(service_impl))
          .is_none());
        Ok((Service::Running(service), replaced, error_payload))
      }
      ServiceState::Stopped(_) => {
        let replaced = (self.services)
          .remove(&*name)
          .map(|(_name, service)| service.into_impl());
        assert!(self.services.insert(name.clone(), service_state).is_none());
        let service = self.services.get(&*name).unwrap();
        Ok((
          Service::Stopped(StoppedService::from_ref(service)),
          replaced,
          error_payload,
        ))
      }
    }
  }

  pub async fn hot_update(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: DirSource,
    config: Config,
  ) -> Result<(RunningService, ServiceImpl)> {
    match self.get(&*name) {
      Some(x) if x.is_stopped() => return Err(ErrorKind::ServiceStopped { name }.into()),
      None => return Err(ErrorKind::ServiceNotFound { name }.into()),
      _ => {}
    }

    let name2 = name.clone();
    let service_impl = sandbox_pool
      .scope(move |sandbox| async move {
        let (service_impl, local_env, internal) =
          prepare_service(&sandbox, name2, uuid, source, config).await?;
        let service_impl = Arc::new(service_impl);
        sandbox
          .create_service(
            service_impl.name(),
            service_impl.downgrade(),
            local_env,
            internal,
            true,
          )
          .await?;
        Ok::<_, crate::Error>(service_impl)
      })
      .await?;

    let service = service_impl.downgrade();
    let replaced = (self.services)
      .remove(&*name)
      .map(|(_name, service)| service.into_impl())
      .unwrap();
    assert!(self
      .services
      .insert(name, ServiceState::Running(service_impl))
      .is_none());

    Ok((service, replaced))
  }
}
