use crate::lua::crypto::create_preload_crypto;
use crate::lua::fs::create_preload_fs;
use crate::lua::http::create_preload_http;
use crate::lua::json::create_preload_json;
use crate::lua::print::create_fn_print;
use crate::lua::LuaTableExt;
use crate::source::{Source, SourceUserData};
use mlua::{Function, Lua, Table};
use std::path::PathBuf;

pub(super) async fn create_isolate<'a, 'b>(
  lua: &'a Lua,
  service_name: &'b str,
  local_storage_path: impl Into<PathBuf>,
  source: Source,
) -> mlua::Result<(Table<'a>, Table<'a>)> {
  let isolate_fn = lua.named_registry_value::<_, Function>("isolate_fn")?;
  let (local_env, internal): (Table, Table) = isolate_fn.call(())?;

  local_env.raw_set("print", create_fn_print(lua, service_name)?)?;
  internal.raw_set("source", SourceUserData(source.clone()))?;

  let preload: Table = internal.raw_get_path("<internal>", &["package", "preload"])?;
  let fs_preload = create_preload_fs(lua, local_storage_path, source).await?;
  preload.raw_set("fs", fs_preload)?;
  preload.raw_set("http", create_preload_http(lua)?)?;
  preload.raw_set("json", create_preload_json(lua)?)?;
  preload.raw_set("crypto", create_preload_crypto(lua)?)?;

  Ok((local_env, internal))
}
