use crate::permission::{Permission, PermissionSet};
use mlua::{ExternalResult, Function, Lua, String as LuaString};
use std::borrow::Cow;
use std::sync::Arc;

pub fn create_fn_os_getenv(lua: &Lua, permissions: Arc<PermissionSet>) -> mlua::Result<Function> {
  lua.create_function(move |_lua, name: LuaString| {
    let name = std::str::from_utf8(name.as_bytes()).to_lua_err()?;
    permissions.check(&Permission::Env(Cow::Borrowed(name)))?;
    std::env::var(name).to_lua_err()
  })
}
