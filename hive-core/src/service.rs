use crate::error::Result;
use crate::lua::Sandbox;
use crate::path::PathMatcher;
use crate::permission::PermissionSet;
use crate::source::Source;
use crate::task::Pool;
use crate::ErrorKind::*;
use dashmap::DashSet;
use serde::ser::SerializeStruct;
use serde::Serialize;
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, Weak};
use uuid::Uuid;

#[derive(Debug)]
struct ServiceImpl {
  name: Box<str>,
  paths: Vec<PathMatcher>,
  source: Source,
  permissions: Arc<PermissionSet>,
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
  pub fn permissions(&self) -> &PermissionSet { &self.inner.permissions }
  pub fn uuid(&self) -> Uuid { self.inner.uuid }

  pub(crate) fn permissions_arc(&self) -> Arc<PermissionSet> { self.inner.permissions.clone() }
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

pub struct ServicePool {
  services: DashSet<Arc<ServiceImpl>>,
}

impl ServicePool {
  pub fn new() -> Self {
    Self {
      services: DashSet::new(),
    }
  }

  /// Creates a new service from source, replacing the old one.
  pub async fn create(
    &self,
    sandbox_pool: &Pool<Sandbox>,
    name: String,
    source: Source,
    permissions: PermissionSet,
  ) -> Result<Service> {
    match self.remove(sandbox_pool, &name).await {
      Ok(_) => (),
      Err(error) if matches!(error.kind(), ServiceNotFound(_)) => (),
      Err(error) => return Err(error),
    }

    let service_impl = sandbox_pool
      .scope(move |sandbox| async move {
        let permissions = Arc::new(permissions);
        let (paths, local_env, internal) = sandbox
          .pre_create_service(&name, source.clone(), permissions.clone())
          .await?;
        let service_impl = Arc::new(ServiceImpl {
          name: name.into_boxed_str(),
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
    assert!(self.services.insert(service_impl));
    Ok(service)
  }

  pub async fn get(&self, name: &str) -> Option<Service> {
    self.services.get::<Str>(name.into()).map(|x| x.downgrade())
  }

  pub async fn list(&self) -> Vec<Service> {
    self.services.iter().map(|x| x.downgrade()).collect()
  }

  // TODO: gracefully
  pub async fn remove(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<ServiceGuard<'_>> {
    if let Some(old_service_impl) = self.services.remove(<&Str>::from(&*name)) {
      let old_service_impl_clone = old_service_impl.clone();
      sandbox_pool
        .scope(move |sandbox| async move {
          sandbox.run_stop(old_service_impl_clone.downgrade()).await?;
          Ok::<_, crate::Error>(())
        })
        .await?;
      Ok(ServiceGuard {
        inner: old_service_impl,
        _p: PhantomData,
      })
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }
}
