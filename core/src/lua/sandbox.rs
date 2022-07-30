use super::abel::side_effect_abel;
use super::crypto::create_preload_crypto;
use super::fs::create_preload_fs;
use super::global_env::modify_global_env;
use super::http::create_preload_http;
use super::isolate::{Isolate, IsolateBuilder};
use super::json::create_preload_json;
use super::logging::side_effect_log;
use super::lua_std::{
  create_preload_coroutine, create_preload_io, create_preload_math, create_preload_os,
  create_preload_string, create_preload_table, create_preload_utf8, side_effect_global_whitelist,
};
use super::stream::create_preload_stream;
use crate::source::Source;
use mlua::{FromLuaMulti, Lua, Table, ToLuaMulti};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Sandbox {
  lua: Lua,
}

impl Sandbox {
  pub fn new() -> mlua::Result<Self> {
    let lua = Lua::new();
    modify_global_env(&lua)?;
    Ok(Self { lua })
  }

  pub fn lua(&self) -> &Lua {
    &self.lua
  }

  pub async fn create_isolate(
    &self,
    name: &str,
    source: Source,
    lsp: impl Into<PathBuf>,
  ) -> mlua::Result<Isolate> {
    let lsp: Arc<Path> = lsp.into().into();
    self
      .create_isolate_builder(source.clone())?
      .add_side_effect(side_effect_global_whitelist)?
      .add_side_effect(side_effect_abel)?
      .add_side_effect(side_effect_log(name))?
      // Lua std, modified
      .add_lib("math", create_preload_math)?
      .add_lib("string", create_preload_string)?
      .add_lib("table", create_preload_table)?
      .add_lib("coroutine", create_preload_coroutine)?
      .add_lib("os", create_preload_os(lsp.clone()))?
      .add_lib("utf8", create_preload_utf8)?
      .add_lib("io", create_preload_io(source.clone(), lsp.clone()))?
      // Abel std (?)
      .add_lib("fs", create_preload_fs(source, lsp))?
      .add_lib("http", create_preload_http)?
      .add_lib("json", create_preload_json)?
      .add_lib("crypto", create_preload_crypto)?
      .add_lib("stream", create_preload_stream)?
      .add_lua_lib("testing", include_str!("lib/testing.lua"))?
      // ...and load some of then into local env
      .load_libs(["math", "string", "table", "coroutine", "os", "utf8", "io"])?
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
    use mlua::ChunkMode;

    let env: Table = self.get_local_env(isolate)?;
    (self.lua.load(chunk))
      .set_mode(ChunkMode::Text)
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
