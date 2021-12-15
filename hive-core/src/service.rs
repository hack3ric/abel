use crate::source::Source;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub(crate) struct ServiceImpl {
  name: Box<str>,
  paths: Vec<Box<str>>,
  source: Source,
  uuid: Uuid,
}

impl Hash for ServiceImpl {
  fn hash<H: Hasher>(&self, state: &mut H) { self.name.hash(state); }
}

pub struct Service {
  inner: Arc<ServiceImpl>,
}

impl Service {
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

  pub fn create_service(&self, name: impl AsRef<str>, source: Source) {
    todo!("Implement this after `LuaSandbox`")
  }
}
