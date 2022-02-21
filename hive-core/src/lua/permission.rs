use crate::permission::{Permission, PermissionSet};
use mlua::UserData;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PermissionBridge(pub(crate) Arc<PermissionSet>);

impl UserData for PermissionBridge {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_method("check", |_lua, this, perm: Permission| {
      Ok(this.0.check_ok(&perm))
    })
  }
}
