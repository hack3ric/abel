use crate::error::Result;
use crate::lua::Sandbox;
use crate::object_pool::Pool;
use crate::path::PathMatcher;
use crate::source::Source;
use crate::ErrorKind::*;
use serde::ser::SerializeStruct;
use serde::Serialize;
use std::borrow::Borrow;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug)]
struct ServiceImpl {
  name: Box<str>,
  paths: Vec<PathMatcher>,
  source: Source,
  uuid: Uuid,
}

impl Hash for ServiceImpl {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.name.hash(state);
  }
}

impl PartialEq for ServiceImpl {
  fn eq(&self, other: &Self) -> bool {
    self.name == other.name
  }
}

impl Eq for ServiceImpl {}

impl Borrow<Str> for Arc<ServiceImpl> {
  fn borrow(&self) -> &Str {
    self.name.as_ref().into()
  }
}

impl ServiceImpl {
  fn downgrade(self: &Arc<Self>) -> Service {
    Service {
      inner: Arc::downgrade(self),
    }
  }
}

/// Helper struct for implementing `Borrow` for `Arc<ServiceImpl>`.
#[derive(Hash, PartialEq, Eq)]
pub(crate) struct Str(str);

impl<'a> From<&'a str> for &'a Str {
  fn from(x: &str) -> &Str {
    unsafe { &*(x as *const str as *const Str) }
  }
}

/// A reference to an inner service.
#[derive(Debug, Clone)]
pub struct Service {
  inner: Weak<ServiceImpl>,
}

impl Service {
  pub fn try_upgrade(&self) -> Result<ServiceGuard<'_>> {
    Ok(ServiceGuard {
      inner: self.inner.upgrade().ok_or(ServiceDropped)?,
      _p: PhantomData,
    })
  }

  pub fn upgrade(&self) -> ServiceGuard<'_> {
    self.try_upgrade().unwrap()
  }

  pub fn is_dropped(&self) -> bool {
    self.inner.strong_count() == 0
  }

  pub fn ptr_eq(&self, other: &Self) -> bool {
    self.inner.ptr_eq(&other.inner)
  }
}

/// An RAII guard of shared reference to an inner service.
///
/// Used to get information of this service.
pub struct ServiceGuard<'a> {
  inner: Arc<ServiceImpl>,
  _p: PhantomData<&'a ()>,
}

#[rustfmt::skip]
impl ServiceGuard<'_> {
  pub fn name(&self) -> &str { &self.inner.name }
  pub fn paths(&self) -> &[PathMatcher] { &self.inner.paths }
  pub fn source(&self) -> &Source { &self.inner.source }
  pub fn uuid(&self) -> Uuid { self.inner.uuid }
}

impl Serialize for ServiceGuard<'_> {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let mut x = serializer.serialize_struct("Service", 3)?;
    x.serialize_field("name", self.name())?;
    x.serialize_field("paths", self.paths())?;
    x.serialize_field("uuid", &self.uuid())?;
    x.end()
  }
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

  /// Creates a new service from source, replacing the old one.
  pub async fn create_service(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: impl Into<String>,
    source: Source,
  ) -> Result<Service> {
    let name = name.into();
    let mut services = self.services.write().await;

    if let Some(old_service_impl) = services.take(<&Str>::from(&*name)) {
      sandbox_pool
        .scope(move |mut sandbox| async move {
          sandbox.run_stop(old_service_impl.downgrade()).await?;
          Ok::<_, crate::Error>(())
        })
        .await
        .unwrap()?;
    }

    let service_impl = sandbox_pool
      .scope(move |mut sandbox| async move {
        let (paths, local_env, internal) =
          sandbox.pre_create_service(&name, source.clone()).await?;
        let service_impl = Arc::new(ServiceImpl {
          name: name.into_boxed_str(),
          paths,
          source,
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
      .await
      .unwrap()?;
    let service = service_impl.downgrade();
    assert!(services.insert(service_impl));
    Ok(service)
  }

  pub async fn get_service(&self, name: impl AsRef<str>) -> Option<Service> {
    self
      .services
      .read()
      .await
      .get::<Str>(name.as_ref().into())
      .map(ServiceImpl::downgrade)
  }

  pub async fn list(&self) -> Vec<Service> {
    self
      .services
      .read()
      .await
      .iter()
      .map(|x| x.downgrade())
      .collect()
  }
}
