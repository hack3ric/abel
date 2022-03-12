use crate::error::Result;
use crate::lua::Sandbox;
use crate::path::PathMatcher;
use crate::permission::PermissionSet;
use crate::source::Source;
use crate::task::Pool;
use crate::util::MyStr;
use crate::Config;
use crate::ErrorKind::*;
use dashmap::DashSet;
use serde::Serialize;
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, Weak};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
struct ServiceImpl {
  name: Box<str>,
  pkg_name: Option<String>,
  description: Option<String>,
  paths: Vec<PathMatcher>,
  #[serde(skip)]
  source: Source,
  #[serde(serialize_with = "crate::util::serialize_arc")]
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

impl Borrow<MyStr> for Arc<ServiceImpl> {
  fn borrow(&self) -> &MyStr {
    self.name.as_ref().into()
  }
}

impl ServiceImpl {
  fn downgrade(self: &Arc<Self>) -> LiveService {
    LiveService {
      inner: Arc::downgrade(self),
    }
  }
}

/// A reference to an inner service.
#[derive(Debug, Clone)]
pub struct LiveService {
  inner: Weak<ServiceImpl>,
}

impl LiveService {
  pub fn try_upgrade(&self) -> Result<LiveServiceGuard<'_>> {
    Ok(LiveServiceGuard {
      inner: self.inner.upgrade().ok_or(ServiceDropped)?,
      _p: PhantomData,
    })
  }

  pub fn upgrade(&self) -> LiveServiceGuard<'_> {
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
pub struct LiveServiceGuard<'a> {
  inner: Arc<ServiceImpl>,
  _p: PhantomData<&'a ()>,
}

#[rustfmt::skip]
impl LiveServiceGuard<'_> {
  pub fn name(&self) -> &str { &self.inner.name }
  pub fn pkg_name(&self) -> Option<&str> { self.inner.pkg_name.as_deref() }
  pub fn description(&self) -> Option<&str> { self.inner.description.as_deref() }
  pub fn paths(&self) -> &[PathMatcher] { &self.inner.paths }
  pub fn source(&self) -> &Source { &self.inner.source }
  pub fn permissions(&self) -> &PermissionSet { &self.inner.permissions }
  pub fn uuid(&self) -> Uuid { self.inner.uuid }

  pub(crate) fn permissions_arc(&self) -> Arc<PermissionSet> { self.inner.permissions.clone() }
}

impl Serialize for LiveServiceGuard<'_> {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    self.inner.as_ref().serialize(serializer)
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
    assert!(self.services.insert(service_impl));
    Ok(service)
  }

  pub async fn get(&self, name: &str) -> Option<LiveService> {
    self
      .services
      .get::<MyStr>(name.into())
      .map(|x| x.downgrade())
  }

  pub async fn list(&self) -> Vec<LiveService> {
    self.services.iter().map(|x| x.downgrade()).collect()
  }

  // TODO: gracefully
  pub async fn remove(&self, sandbox_pool: &Pool<Sandbox>, name: &str) -> Result<LiveServiceGuard<'_>> {
    if let Some(old_service_impl) = self.services.remove(MyStr::new(&*name)) {
      let old_service_impl_clone = old_service_impl.clone();
      sandbox_pool
        .scope(move |sandbox| async move {
          sandbox.run_stop(old_service_impl_clone.downgrade(), true).await?;
          Ok::<_, crate::Error>(())
        })
        .await?;
      Ok(LiveServiceGuard {
        inner: old_service_impl,
        _p: PhantomData,
      })
    } else {
      Err(ServiceNotFound(name.into()).into())
    }
  }
}
