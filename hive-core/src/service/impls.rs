use crate::path::PathMatcher;
use crate::permission::PermissionSet;
use crate::util::MyStr;
use crate::ErrorKind::ServiceDropped;
use crate::{Result, Source};
use serde::Serialize;
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, Weak};
use uuid::Uuid;

#[derive(Serialize)]
#[serde(untagged)]
pub enum ServiceState {
  Running(#[serde(serialize_with = "crate::util::serialize_arc")] Arc<ServiceImpl>),
  Stopped(ServiceImpl),
}

impl ServiceState {
  pub fn name(&self) -> &str {
    match self {
      Self::Running(x) => &x.name,
      Self::Stopped(x) => &x.name,
    }
  }

  pub fn into_impl(self) -> ServiceImpl {
    match self {
      Self::Running(x) => Arc::try_unwrap(x).unwrap_or_else(|arc| arc.as_ref().clone()),
      Self::Stopped(x) => x,
    }
  }
}

impl Hash for ServiceState {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.name().hash(state);
  }
}

impl PartialEq for ServiceState {
  fn eq(&self, other: &Self) -> bool {
    self.name() == other.name()
  }
}

impl Eq for ServiceState {}

impl Borrow<MyStr> for ServiceState {
  fn borrow(&self) -> &MyStr {
    self.name().into()
  }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceImpl {
  pub(crate) name: Box<str>,
  pub(crate) pkg_name: Option<String>,
  pub(crate) description: Option<String>,
  pub(crate) paths: Vec<PathMatcher>,
  #[serde(skip)]
  pub(crate) source: Source,
  #[serde(serialize_with = "crate::util::serialize_arc")]
  pub(crate) permissions: Arc<PermissionSet>,
  pub(crate) uuid: Uuid,
}

impl ServiceImpl {
  pub(crate) fn downgrade(self: &Arc<Self>) -> RunningService {
    RunningService {
      inner: Arc::downgrade(self),
    }
  }

  pub fn uuid(&self) -> &Uuid {
    &self.uuid
  }
}

/// A reference to an inner service.
#[derive(Debug, Clone)]
pub struct RunningService {
  inner: Weak<ServiceImpl>,
}

impl RunningService {
  pub fn try_upgrade(&self) -> Result<RunningServiceGuard<'_>> {
    Ok(RunningServiceGuard {
      inner: self.inner.upgrade().ok_or(ServiceDropped)?,
      _p: PhantomData,
    })
  }

  pub fn upgrade(&self) -> RunningServiceGuard<'_> {
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
pub struct RunningServiceGuard<'a> {
  pub(crate) inner: Arc<ServiceImpl>,
  pub(crate) _p: PhantomData<&'a ()>,
}

#[rustfmt::skip]
impl RunningServiceGuard<'_> {
  pub fn name(&self) -> &str { &self.inner.name }
  pub fn pkg_name(&self) -> Option<&str> { self.inner.pkg_name.as_deref() }
  pub fn description(&self) -> Option<&str> { self.inner.description.as_deref() }
  pub fn paths(&self) -> &[PathMatcher] { &self.inner.paths }
  pub fn source(&self) -> &Source { &self.inner.source }
  pub fn permissions(&self) -> &PermissionSet { &self.inner.permissions }
  pub fn uuid(&self) -> Uuid { self.inner.uuid }

  pub(crate) fn permissions_arc(&self) -> Arc<PermissionSet> { self.inner.permissions.clone() }
}

impl Serialize for RunningServiceGuard<'_> {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    self.inner.as_ref().serialize(serializer)
  }
}
