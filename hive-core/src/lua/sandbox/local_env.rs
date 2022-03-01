use crate::lua::context::create_context;
use crate::lua::http::create_module_request;
use crate::lua::modules::{create_module_fs, create_module_permission};
use crate::lua::LuaTableExt;
use crate::permission::PermissionSet;
use crate::{Result, Source};
use mlua::{Function, Lua, Table};
use std::sync::Arc;

pub(super) fn create_local_env<'a>(
  lua: &'a Lua,
  service_name: &str,
  source: Source,
  permissions: Arc<PermissionSet>,
) -> Result<(Table<'a>, Table<'a>)> {
  let local_env_fn = lua.named_registry_value::<_, Function>("local_env_fn")?;
  let (local_env, internal): (Table, Table) = local_env_fn.call(())?;

  let hive: Table = local_env.raw_get("hive")?;
  hive.raw_set(
    "context",
    lua.create_ser_userdata(create_context(service_name.into()))?,
  )?;
  hive.raw_set(
    "permission",
    create_module_permission(lua, permissions.clone())?,
  )?;

  internal.raw_set("source", source.clone())?;

  let preload: Table = internal.raw_get_path("<internal>", &["package", "preload"])?;
  preload.raw_set("request", create_module_request(lua, permissions)?)?;
  preload.raw_set("fs", create_module_fs(lua, source)?)?;

  Ok((local_env, internal))
}
