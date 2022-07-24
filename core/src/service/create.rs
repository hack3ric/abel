use super::{
  get_local_storage_path, RunningService, Service, ServiceImpl, ServiceName, ServicePool,
  ServiceState, StoppedService,
};
use crate::lua::isolate::Isolate;
use crate::pool::RuntimePool;
use crate::runtime::Runtime;
use crate::source::Source;
use crate::ErrorKind::{self, ServiceNotFound, ServiceStopped};
use crate::{Config, Error, Result};
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
  rt: &Runtime,
  name: ServiceName,
  uuid: Option<Uuid>,
  source: Source,
  config: Config,
) -> Result<(ServiceImpl, Isolate)> {
  let Config {
    pkg_name,
    description,
  } = config;
  let (paths, isolate) = rt.prepare_service(&name, source.clone()).await?;
  let service_impl = ServiceImpl {
    name,
    pkg_name,
    description,
    paths,
    source,
    uuid: uuid.unwrap_or_else(Uuid::new_v4),
  };
  Ok((service_impl, isolate))
}

impl ServicePool {
  pub async fn load(
    &self,
    rt_pool: &RuntimePool,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: Source,
    config: Config,
  ) -> Result<(StoppedService<'_>, Option<ServiceImpl>, ErrorPayload)> {
    let services = self.services.clone();
    let name2 = name.clone();
    let (service_impl, error_payload) = rt_pool
      .scope(move |rt| async move {
        let mut error_payload = ErrorPayload::empty();

        let (service_impl, isolate) =
          prepare_service(&rt, name2.clone(), uuid, source, config).await?;
        rt.remove_isolate(isolate)?;

        match Self::scope_stop(services, &rt, &*name2).await {
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
    rt_pool: &RuntimePool,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: Source,
    config: Config,
  ) -> Result<(Service<'_>, Option<ServiceImpl>, ErrorPayload)> {
    let services = self.services.clone();
    let state = self.state.clone();
    let name2 = name.clone();
    let (service_state, error_payload) = rt_pool
      .scope(move |rt| async move {
        let mut error_payload = ErrorPayload::default();

        let local_storage_path = get_local_storage_path(&state, &name2);
        if !local_storage_path.exists() {
          tokio::fs::create_dir(&local_storage_path).await?;
        }
        let (service_impl, isolate) =
          prepare_service(&rt, name2.clone(), uuid, source, config).await?;

        match Self::scope_stop(services, &rt, &*name2).await {
          Ok(_) => {}
          Err(error) if matches!(error.kind(), ServiceStopped { .. } | ServiceNotFound { .. }) => {}
          Err(error) => error_payload.stop = Some(error),
        }

        let service_impl = Arc::new(service_impl);
        let result = rt
          .create_service(
            service_impl.name(),
            service_impl.downgrade(),
            isolate,
            false,
          )
          .await;
        let state = ServiceState::Running(service_impl);
        let state = match result {
          Ok(()) => state,
          Err(err) => {
            error_payload.start = Some(err);
            let service_impl = state.into_impl();
            rt.expire_registry_values();
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
    rt_pool: &RuntimePool,
    name: ServiceName,
    uuid: Option<Uuid>,
    source: Source,
    config: Config,
  ) -> Result<(RunningService, ServiceImpl)> {
    match self.get(&*name) {
      Some(x) if x.is_stopped() => return Err(ErrorKind::ServiceStopped { name }.into()),
      None => return Err(ErrorKind::ServiceNotFound { name }.into()),
      _ => {}
    }

    let name2 = name.clone();
    let service_impl = rt_pool
      .scope(move |rt| async move {
        let (service_impl, isolate) = prepare_service(&rt, name2, uuid, source, config).await?;
        let service_impl = Arc::new(service_impl);
        rt.create_service(service_impl.name(), service_impl.downgrade(), isolate, true)
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
