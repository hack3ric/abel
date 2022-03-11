use crate::permission::{Permission, PermissionSet};
use mlua::{ExternalResult, Function, Lua};
use std::sync::Arc;

pub fn create_fn_os_getenv(lua: &Lua, permissions: Arc<PermissionSet>) -> mlua::Result<Function> {
  lua.create_function(move |_lua, name: String| {
    permissions.check(&Permission::env(name.clone()))?;
    std::env::var(name).to_lua_err()
  })
}
