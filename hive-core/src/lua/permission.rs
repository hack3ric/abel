use crate::permission::{Permission, PermissionSet};
use mlua::Lua;
use std::sync::Arc;

pub fn create_module_permission(
  lua: &Lua,
  permissions: Arc<PermissionSet>,
) -> mlua::Result<mlua::Table> {
  let permission_table = lua.create_table()?;
  permission_table.raw_set(
    "check",
    lua.create_function(move |_lua, perm: Permission| Ok(permissions.clone().check_ok(&perm)))?,
  )?;
  Ok(permission_table)
}
