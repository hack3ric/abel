use abel_core::service::{Service, ServiceGuard, ServiceInfo};
use ouroboros::self_referencing;
use serde::{Deserialize, Serialize, Serializer};
use serde_with::skip_serializing_none;
use std::borrow::Cow;

#[self_referencing]
pub struct OwnedServiceWithStatus<'a> {
  service: Service<'a>,
  #[borrows(service)]
  #[covariant]
  guard: ServiceGuard<'this>,
  #[borrows(guard)]
  #[covariant]
  info: ServiceWithStatus<'this>,
}

impl<'a> From<Service<'a>> for OwnedServiceWithStatus<'a> {
  fn from(service: Service<'a>) -> Self {
    OwnedServiceWithStatusBuilder {
      service,
      guard_builder: |x| x.upgrade(),
      info_builder: |x| ServiceWithStatus::from_guard(x),
    }
    .build()
  }
}

impl Serialize for OwnedServiceWithStatus<'_> {
  fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
    self.borrow_info().serialize(ser)
  }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ServiceStatus {
  #[serde(rename = "running")]
  Running,
  #[serde(rename = "stopped")]
  Stopped,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceWithStatus<'a> {
  pub status: ServiceStatus,
  pub service: Cow<'a, ServiceInfo>,
}

impl<'a> ServiceWithStatus<'a> {
  pub fn from_guard<'b: 'a>(guard: &'b ServiceGuard<'a>) -> Self {
    use ServiceStatus::*;
    match guard {
      ServiceGuard::Running { service } => Self {
        status: Running,
        service: Cow::Borrowed(service.info()),
      },
      ServiceGuard::Stopped { service } => Self {
        status: Stopped,
        service: Cow::Borrowed(service.info()),
      },
    }
  }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[skip_serializing_none]
pub struct ErrorPayload<'a> {
  pub start: Option<Cow<'a, str>>,
  pub stop: Option<Cow<'a, str>>,
}

impl ErrorPayload<'_> {
  fn is_empty(&self) -> bool {
    self.start.is_none() && self.stop.is_none()
  }
}

impl<'a> From<abel_core::service::ErrorPayload> for ErrorPayload<'a> {
  fn from(payload: abel_core::service::ErrorPayload) -> Self {
    Self {
      start: payload.start.map(|x| x.to_string().into()),
      stop: payload.stop.map(|x| x.to_string().into()),
    }
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpUploadResponse<'a> {
  pub new_service: ServiceWithStatus<'a>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub replaced_service: Option<Cow<'a, ServiceInfo>>,
  #[serde(default, skip_serializing_if = "ErrorPayload::is_empty")]
  pub errors: ErrorPayload<'a>,
}
