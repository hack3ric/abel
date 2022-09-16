use super::crypto::create_preload_crypto;
use super::fs::create_preload_fs;
use super::global_env::modify_global_env;
use super::http::create_preload_http;
use super::isolate::{Isolate, IsolateBuilder};
use super::json::create_preload_json;
use super::lua_std::{
  create_preload_coroutine, create_preload_math, create_preload_os, create_preload_string,
  create_preload_table, create_preload_utf8, side_effect_global_whitelist,
};
use super::require::RemoteInterface;
use super::sanitize_error;
use super::stream::create_preload_stream;
use crate::source::Source;
use crate::Result;
use mlua::{FromLuaMulti, Lua, Table, ToLuaMulti};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Sandbox {
  lua: Lua,
  remote: RemoteInterface,
}

impl Sandbox {
  pub fn new(remote: RemoteInterface) -> mlua::Result<Self> {
    let lua = Lua::new();
    modify_global_env(&lua)?;
    Ok(Self { lua, remote })
  }

  pub fn lua(&self) -> &Lua {
    &self.lua
  }

  pub fn isolate_builder(&self, source: Source) -> mlua::Result<IsolateBuilder> {
    IsolateBuilder::new(&self.lua, source, self.remote.clone())
  }

  pub fn isolate_builder_with_stdlib(
    &self,
    source: Source,
    lsp: impl Into<PathBuf>,
  ) -> mlua::Result<IsolateBuilder> {
    let lsp: Arc<Path> = lsp.into().into();
    self
      .isolate_builder(source.clone())?
      .add_side_effect(side_effect_global_whitelist)?
      // Lua std, modified
      .add_lib("math", create_preload_math)?
      .add_lib("string", create_preload_string)?
      .add_lib("table", create_preload_table)?
      .add_lib("coroutine", create_preload_coroutine)?
      .add_lib("os", create_preload_os)?
      .add_lib("utf8", create_preload_utf8)?
      // Abel std (?)
      .add_lib("fs", create_preload_fs(source, lsp))?
      .add_lib("http", create_preload_http)?
      .add_lib("json", create_preload_json)?
      .add_lib("crypto", create_preload_crypto)?
      .add_lib("stream", create_preload_stream)?
      .add_lua_lib("testing", include_str!("testing.lua"))?
      // ...and load some of then into local env
      .load_libs(["math", "string", "table", "coroutine", "os", "utf8"])
  }

  pub async fn run_isolate<'lua, A: ToLuaMulti<'lua>, R: FromLuaMulti<'lua>>(
    &'lua self,
    isolate: &Isolate,
    path: &str,
    args: A,
  ) -> Result<R> {
    let env: Table = self.get_local_env(isolate)?;
    let result = isolate
      .source
      .load(&self.lua, path, env)
      .await?
      .call_async(args)
      .await
      .map_err(sanitize_error)?;
    Ok(result)
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
