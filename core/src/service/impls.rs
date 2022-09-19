use super::ServiceName;
use crate::path::PathMatcher;
use crate::source::Source;
use crate::ErrorKind::ServiceDropped;
use crate::Result;
use dashmap::mapref::multiple::RefMulti;
use dashmap::mapref::one::Ref;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, Weak};
use uuid::Uuid;

pub(super) enum ServiceState {
  Running(Arc<ServiceImpl>),
  Stopped(ServiceImpl),
}

impl ServiceState {
  pub fn into_impl(self) -> ServiceImpl {
    match self {
      Self::Running(x) => Arc::try_unwrap(x).unwrap_or_else(|arc| arc.as_ref().clone()),
      Self::Stopped(x) => x,
    }
  }
}

#[derive(Debug, Clone)]
pub struct ServiceImpl {
  pub(crate) info: ServiceInfo,
  pub(crate) source: Source,
}

impl ServiceImpl {
  pub(crate) fn downgrade(self: &Arc<Self>) -> RunningService {
    RunningService {
      inner: Arc::downgrade(self),
    }
  }

  pub fn info(&self) -> &ServiceInfo {
    &self.info
  }

  pub fn source(&self) -> &Source {
    &self.source
  }
}

impl Deref for ServiceImpl {
  type Target = ServiceInfo;

  fn deref(&self) -> &Self::Target {
    &self.info
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
  pub(crate) name: ServiceName,
  pub(crate) pkg_name: Option<String>,
  pub(crate) description: Option<String>,
  pub(crate) paths: Vec<PathMatcher>,
  pub(crate) uuid: Uuid,
}

#[rustfmt::skip]
impl ServiceInfo {
  pub fn name(&self) -> &str { &self.name }
  pub fn pkg_name(&self) -> Option<&str> { self.pkg_name.as_deref() }
  pub fn description(&self) -> Option<&str> { self.description.as_deref() }
  pub fn paths(&self) -> &[PathMatcher] { &self.paths }
  pub fn uuid(&self) -> Uuid { self.uuid }
}

pub enum Service<'a> {
  Running(RunningService),
  Stopped(StoppedService<'a>),
}

impl Service<'_> {
  pub fn try_upgrade(&self) -> Result<ServiceGuard<'_>> {
    Ok(match self {
      Service::Running(x) => ServiceGuard::Running {
        service: x.try_upgrade()?,
      },
      Service::Stopped(service) => ServiceGuard::Stopped { service },
    })
  }

  pub fn upgrade(&self) -> ServiceGuard<'_> {
    self.try_upgrade().unwrap()
  }

  pub fn is_running(&self) -> bool {
    matches!(self, Self::Running(_))
  }

  pub fn is_stopped(&self) -> bool {
    matches!(self, Self::Stopped(_))
  }
}

pub enum ServiceGuard<'a> {
  Running { service: RunningServiceGuard<'a> },
  Stopped { service: &'a ServiceImpl },
}

impl Deref for ServiceGuard<'_> {
  type Target = ServiceImpl;

  fn deref(&self) -> &ServiceImpl {
    match self {
      Self::Running { service } => &**service,
      Self::Stopped { service } => service,
    }
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

impl Deref for RunningServiceGuard<'_> {
  type Target = ServiceImpl;

  fn deref(&self) -> &ServiceImpl {
    &self.inner
  }
}

impl Serialize for RunningServiceGuard<'_> {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    self.inner.as_ref().serialize(serializer)
  }
}

pub struct StoppedService<'a>(StoppedServiceInner<'a>);

enum StoppedServiceInner<'a> {
  Ref(Ref<'a, ServiceName, ServiceState>),
  RefMulti(RefMulti<'a, ServiceName, ServiceState>),
}

impl<'a> StoppedService<'a> {
  pub(super) fn from_ref(x: Ref<'a, ServiceName, ServiceState>) -> Self {
    assert!(matches!(x.value(), ServiceState::Stopped(_)));
    Self(StoppedServiceInner::Ref(x))
  }

  pub(super) fn from_ref_multi(x: RefMulti<'a, ServiceName, ServiceState>) -> Self {
    assert!(matches!(x.value(), ServiceState::Stopped(_)));
    Self(StoppedServiceInner::RefMulti(x))
  }
}

impl Deref for StoppedService<'_> {
  type Target = ServiceImpl;

  fn deref(&self) -> &ServiceImpl {
    match &self.0 {
      StoppedServiceInner::Ref(x) => {
        if let ServiceState::Stopped(x) = x.value() {
          x
        } else {
          unreachable!()
        }
      }
      StoppedServiceInner::RefMulti(x) => {
        if let ServiceState::Stopped(x) = x.value() {
          x
        } else {
          unreachable!()
        }
      }
    }
  }
}
