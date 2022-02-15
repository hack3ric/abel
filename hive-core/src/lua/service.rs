use crate::{Service, ServiceGuard};
use mlua::UserData;
use crate::permission::Permission;
use super::modules;

#[derive(Debug, Clone)]
pub struct ServiceBridge(Service);

impl ServiceBridge {
  pub fn upgrade(&self) -> ServiceBridgeGuard {
    ServiceBridgeGuard(self.0.upgrade())
  }
}

pub struct ServiceBridgeGuard<'a>(ServiceGuard<'a>);

impl ServiceBridgeGuard<'_> {
  pub fn check(&self, p: &Permission) -> bool {
    self.0.permissions().check(p)
  }
}

impl UserData for ServiceBridge {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    modules::request::add_methods(methods);
  }
}
