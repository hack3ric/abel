use crate::lua::crypto::create_preload_crypto;
use crate::lua::fs::create_preload_fs;
use crate::lua::http::create_preload_http;
use crate::lua::json::create_preload_json;
use crate::lua::print::create_fn_print;
use crate::lua::LuaTableExt;
use crate::source::{Source, SourceUserData};
use mlua::{Function, Lua, Table, TableExt};
use std::path::PathBuf;

pub(super) async fn create_isolate<'lua, 'a>(
  lua: &'lua Lua,
  service_name: &'a str,
  local_storage_path: impl Into<PathBuf>,
  source: Source,
) -> mlua::Result<(Table<'lua>, Table<'lua>)> {
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

pub struct IsolateBuilder<'lua> {
  lua: &'lua Lua,
  local_env: Table<'lua>,
  internal: Table<'lua>,
  preload: Table<'lua>,
}

impl<'lua> IsolateBuilder<'lua> {
  pub fn new(lua: &'lua Lua) -> mlua::Result<Self> {
    let isolate_fn = lua.named_registry_value::<_, Function>("isolate_fn")?;
    let (local_env, internal): (_, Table) = isolate_fn.call(())?;
    let preload = internal.raw_get_path("<internal>", &["package", "preload"])?;
    Ok(Self {
      lua,
      local_env,
      internal,
      preload,
    })
  }

  pub fn add_side_effect(
    &mut self,
    f: impl FnOnce(&Lua, Table, Table) -> mlua::Result<()>,
  ) -> mlua::Result<&mut Self> {
    f(self.lua, self.local_env.clone(), self.internal.clone())?;
    Ok(self)
  }

  // TODO: How to access local_env in loaders?
  pub fn add_lib(
    &mut self,
    name: &str,
    f: impl FnOnce(&Lua) -> mlua::Result<Function>,
  ) -> mlua::Result<&mut Self> {
    self.preload.raw_set(name, f(self.lua)?)?;
    Ok(self)
  }

  pub fn add_lib_and_load(
    &mut self,
    name: &str,
    f: impl FnOnce(&Lua) -> mlua::Result<Function>,
  ) -> mlua::Result<&mut Self> {
    self.add_lib(name, f)?;
    let lib = self.local_env.call_function("require", name)?;

    Ok(self)
  }

  // pub fn build
}
