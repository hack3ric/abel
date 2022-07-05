use super::create_preload_routing;
use super::crypto::create_preload_crypto;
use super::fs::create_preload_fs;
use super::global_env::modify_global_env;
use super::http::create_preload_http;
use super::isolate::{Isolate, IsolateBuilder};
use super::json::create_preload_json;
use super::lua_std::{
  create_preload_coroutine, create_preload_io, create_preload_math, create_preload_os,
  create_preload_string, create_preload_table, create_preload_utf8, global_whitelist,
};
use super::print::create_fn_print;
use crate::source::{Source, SourceUserData};
use mlua::{FromLuaMulti, Lua, Table, ToLuaMulti};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Sandbox {
  pub(crate) lua: Lua,
}

impl Sandbox {
  pub fn new() -> mlua::Result<Self> {
    let lua = Lua::new();
    modify_global_env(&lua)?;
    Ok(Self { lua })
  }

  pub async fn create_isolate(
    &self,
    name: &str,
    source: Source,
    local_storage_path: impl Into<PathBuf>,
  ) -> mlua::Result<Isolate> {
    let local_storage_path: Arc<Path> = local_storage_path.into().into();
    self
      .create_isolate_builder(source.clone())?
      .add_side_effect(global_whitelist)?
      .add_side_effect(|lua, env, _| env.raw_set("print", create_fn_print(lua, name)?))?
      .add_side_effect(|_, _, i| i.raw_set("source", SourceUserData(source.clone())))?
      // Lua std, modified
      .add_lib("math", create_preload_math)?
      .add_lib("string", create_preload_string)?
      .add_lib("table", create_preload_table)?
      .add_lib("coroutine", create_preload_coroutine)?
      .add_lib("os", create_preload_os(local_storage_path.clone()))?
      .add_lib("utf8", create_preload_utf8)?
      .add_lib(
        "io",
        create_preload_io(source.clone(), local_storage_path.clone()),
      )?
      // Abel std (?)
      .add_lib("routing", create_preload_routing)?
      .add_lib("fs", create_preload_fs(source, local_storage_path))?
      .add_lib("http", create_preload_http)?
      .add_lib("json", create_preload_json)?
      .add_lib("crypto", create_preload_crypto)?
      // ...and load some of then into local env
      .load_libs([
        "math", "string", "table", "coroutine", "os", "utf8", "io", "routing",
      ])?
      .build()
  }

  pub fn create_isolate_builder(&self, source: Source) -> mlua::Result<IsolateBuilder> {
    IsolateBuilder::new(&self.lua, source)
  }

  pub async fn run_isolate<'lua, A: ToLuaMulti<'lua>, R: FromLuaMulti<'lua>>(
    &'lua self,
    isolate: &Isolate,
    path: &str,
    args: A,
  ) -> mlua::Result<R> {
    let env: Table = self.get_local_env(isolate)?;
    isolate
      .source
      .load(&self.lua, path, env)
      .await?
      .call_async(args)
      .await
  }

  #[cfg(test)]
  pub async fn run_isolate_ext<'lua, C, A, R>(
    &'lua self,
    isolate: &Isolate,
    chunk: &C,
    name: &str,
    args: A,
  ) -> mlua::Result<R>
  where
    C: mlua::AsChunk<'lua> + ?Sized,
    A: ToLuaMulti<'lua>,
    R: FromLuaMulti<'lua>,
  {
    let env: Table = self.get_local_env(isolate)?;
    (self.lua.load(chunk))
      .set_environment(env)?
      .set_name(name)?
      .call_async::<A, R>(args)
      .await
  }

  pub fn get_local_env(&self, isolate: &Isolate) -> mlua::Result<Table> {
    self.lua.registry_value(&isolate.local_env)
  }

  pub fn get_internal(&self, isolate: &Isolate) -> mlua::Result<Table> {
    self.lua.registry_value(&isolate.internal)
  }

  pub fn remove_isolate(&self, isolate: Isolate) -> mlua::Result<()> {
    self.lua.remove_registry_value(isolate.local_env)?;
    self.lua.remove_registry_value(isolate.internal)
  }

  pub fn expire_registry_values(&self) {
    self.lua.expire_registry_values();
  }
}
