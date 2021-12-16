use crate::error::Error::*;
use crate::error::HiveResult;
use crate::lua::Sandbox;
use crate::object_pool::Pool;
use crate::source::Source;
use std::backtrace::Backtrace;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug)]
struct ServiceImpl {
  name: Box<str>,
  paths: Vec<Box<str>>,
  source: Source,
  uuid: Uuid,
}

impl Hash for ServiceImpl {
  fn hash<H: Hasher>(&self, state: &mut H) { self.name.hash(state); }
}

impl PartialEq for ServiceImpl {
  fn eq(&self, other: &Self) -> bool { self.name == other.name }
}

impl Eq for ServiceImpl {}

impl ServiceImpl {
  fn downgrade(self: &Arc<Self>) -> Service {
    Service {
      inner: Arc::downgrade(self),
    }
  }
}

/// A reference to an inner service.
#[derive(Debug)]
pub struct Service {
  inner: Weak<ServiceImpl>,
}

impl Service {
  pub fn try_upgrade(&self) -> HiveResult<ServiceGuard<'_>> {
    Ok(ServiceGuard {
      inner: self.inner.upgrade().ok_or(ServiceDropped {
        backtrace: Backtrace::capture(),
      })?,
      _p: PhantomData,
    })
  }

  pub fn upgrade(&self) -> ServiceGuard<'_> { self.try_upgrade().unwrap() }

  pub fn is_dropped(&self) -> bool { self.inner.strong_count() == 0 }
}

/// An RAII guard of shared reference to an inner service.
///
/// Used to get information of this service.
pub struct ServiceGuard<'a> {
  inner: Arc<ServiceImpl>,
  _p: PhantomData<&'a ()>,
}

impl ServiceGuard<'_> {
  fn name(&self) -> &str { &self.inner.name }
  fn paths(&self) -> &[Box<str>] { &self.inner.paths }
  fn source(&self) -> &Source { &self.inner.source }
  fn uuid(&self) -> Uuid { self.inner.uuid }
}

#[derive(Clone)]
pub struct ServicePool {
  services: Arc<RwLock<HashSet<Arc<ServiceImpl>>>>,
}

impl ServicePool {
  pub fn new() -> Self {
    Self {
      services: Arc::new(RwLock::const_new(HashSet::new())),
    }
  }

  pub async fn create_service(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: impl Into<String>,
    source: Source,
  ) -> HiveResult<Service> {
    let name = name.into();
    let service_impl = sandbox_pool
      .scope(move |mut sandbox| async move {
        let paths_with_key = sandbox.pre_create_service(&name, source.clone()).await?;
        let paths = paths_with_key
          .iter()
          .map(|(name, _)| name.clone())
          .collect();
        let service_impl = Arc::new(ServiceImpl {
          name: name.into_boxed_str(),
          paths,
          source,
          uuid: Uuid::new_v4(),
        });
        sandbox
          .finish_create_service(&service_impl.name, service_impl.downgrade(), paths_with_key)
          .await?;
        Ok::<_, crate::Error>(service_impl)
      })
      .await
      .unwrap()?;
    let mut services = self.services.write().await;
    let service = service_impl.downgrade();
    services.insert(service_impl);
    Ok(service)
  }
}
