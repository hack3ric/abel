use crate::lua::context::{create_module_context, SharedTable, SharedTableKey, SharedTableValue};
use crate::lua::env::create_fn_os_getenv;
use crate::lua::fs::create_preload_fs;
use crate::lua::http::create_preload_http;
use crate::lua::json::create_preload_json;
use crate::lua::permission::create_module_permission;
use crate::lua::print::create_fn_print;
use crate::lua::LuaTableExt;
use crate::permission::PermissionSet;
use crate::{HiveState, Result, Source};
use mlua::{Function, Lua, Table};
use std::sync::Arc;

pub(super) async fn create_local_env<'a, 'b>(
  lua: &'a Lua,
  state: &HiveState,
  service_name: &'b str,
  source: Source,
  permissions: Arc<PermissionSet>,
) -> Result<(Table<'a>, Table<'a>)> {
  let local_env_fn = lua.named_registry_value::<_, Function>("local_env_fn")?;
  let (local_env, internal): (Table, Table) = local_env_fn.call(())?;

  local_env.raw_set("print", create_fn_print(lua, service_name)?)?;

  let context = lua.pack(create_module_context(lua, service_name.into())?)?;

  let hive: Table = local_env.raw_get("hive")?;
  hive.raw_set("context", context.clone())?;
  bind_local_env_to_context(lua, local_env.clone(), context)?;
  hive.raw_set(
    "permission",
    create_module_permission(lua, permissions.clone())?,
  )?;

  internal.raw_set("source", source.clone())?;

  let preload: Table = internal.raw_get_path("<internal>", &["package", "preload"])?;
  preload.raw_set(
    "fs",
    create_preload_fs(lua, state, service_name, source, permissions.clone()).await?,
  )?;
  preload.raw_set("http", create_preload_http(lua, permissions.clone())?)?;
  preload.raw_set("json", create_preload_json(lua)?)?;

  let os_module: Table = local_env.raw_get("os")?;
  os_module.raw_set("getenv", create_fn_os_getenv(lua, permissions)?)?;

  Ok((local_env, internal))
}

fn bind_local_env_to_context(lua: &Lua, local_env: Table, context: mlua::Value) -> Result<()> {
  let index = lua
    .create_function(
      |lua, (context, _this, key): (SharedTable, Table, mlua::Value)| {
        if let Ok(key) = lua.unpack::<SharedTableKey>(key) {
          lua.pack(&*context.get(key))
        } else {
          Ok(mlua::Value::Nil)
        }
      },
    )?
    .bind(context.clone())?;

  let newindex = lua
    .create_function(
      |lua, (context, this, key, value): (SharedTable, Table, mlua::Value, mlua::Value)| {
        if let (Ok(key), Ok(value)) = (
          lua.unpack::<SharedTableKey>(key.clone()),
          lua.unpack::<SharedTableValue>(value.clone()),
        ) {
          context.set(key, value);
        } else {
          this.raw_set(key, value)?;
        }
        Ok(())
      },
    )?
    .bind(context)?;

  let mt = lua.create_table_from([("__index", index), ("__newindex", newindex)])?;
  local_env.set_metatable(Some(mt));

  Ok(())
}
