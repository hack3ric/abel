use crate::lua::crypto::create_preload_crypto;
use crate::lua::fs::create_preload_fs;
use crate::lua::http::create_preload_http;
use crate::lua::json::create_preload_json;
use crate::lua::print::create_fn_print;
use crate::lua::shared::{create_module_shared, SharedTable, SharedTableKey, SharedTableValue};
use crate::lua::LuaTableExt;
use crate::source::{Source, SourceUserData};
use crate::{HiveState, Result};
use mlua::{Function, Lua, Table};

pub(super) async fn create_local_env<'a, 'b>(
  lua: &'a Lua,
  state: &HiveState,
  service_name: &'b str,
  source: Source,
) -> Result<(Table<'a>, Table<'a>)> {
  let local_env_fn = lua.named_registry_value::<_, Function>("local_env_fn")?;
  let (local_env, internal): (Table, Table) = local_env_fn.call(())?;

  local_env.raw_set("print", create_fn_print(lua, service_name)?)?;

  let shared = lua.pack(create_module_shared(lua, service_name.into())?)?;

  let hive: Table = local_env.raw_get("hive")?;
  hive.raw_set("shared", shared.clone())?;
  bind_local_env_to_shared(lua, local_env.clone(), shared)?;

  internal.raw_set("source", SourceUserData(source.clone()))?;

  let preload: Table = internal.raw_get_path("<internal>", &["package", "preload"])?;
  let fs_preload = create_preload_fs(lua, state, service_name, source).await?;
  preload.raw_set("fs", fs_preload)?;
  preload.raw_set("http", create_preload_http(lua)?)?;
  preload.raw_set("json", create_preload_json(lua)?)?;
  preload.raw_set("crypto", create_preload_crypto(lua)?)?;

  Ok((local_env, internal))
}

fn bind_local_env_to_shared(lua: &Lua, local_env: Table, shared: mlua::Value) -> Result<()> {
  let index = lua
    .create_function(
      |lua, (shared, _this, key): (SharedTable, Table, mlua::Value)| {
        if let Ok(key) = lua.unpack::<SharedTableKey>(key) {
          lua.pack(&*shared.get(key))
        } else {
          Ok(mlua::Value::Nil)
        }
      },
    )?
    .bind(shared.clone())?;

  let newindex = lua
    .create_function(
      |lua, (shared, this, key, value): (SharedTable, Table, mlua::Value, mlua::Value)| {
        if let (Ok(key), Ok(value)) = (
          lua.unpack::<SharedTableKey>(key.clone()),
          lua.unpack::<SharedTableValue>(value.clone()),
        ) {
          shared.set(key, value);
        } else {
          this.raw_set(key, value)?;
        }
        Ok(())
      },
    )?
    .bind(shared)?;

  let mt = lua.create_table_from([("__index", index), ("__newindex", newindex)])?;
  local_env.set_metatable(Some(mt));

  Ok(())
}
