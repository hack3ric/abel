use super::global_env::modify_global_env;
use super::isolate::create_isolate;
use crate::source::Source;
use mlua::{AsChunk, FromLuaMulti, Lua, RegistryKey, Table, ToLuaMulti};
use std::path::PathBuf;

pub struct Sandbox {
  pub(crate) lua: Lua,
}

impl Sandbox {
  pub fn new() -> mlua::Result<Self> {
    let lua = Lua::new();
    modify_global_env(&lua)?;
    Ok(Self { lua })
  }

  // TODO: maybe make this modular
  pub async fn create_isolate(
    &self,
    name: &str,
    local_storage_path: impl Into<PathBuf>,
    source: Source,
  ) -> mlua::Result<Isolate> {
    let (local_env, internal) =
      create_isolate(&self.lua, name, local_storage_path, source.clone()).await?;
    let local_env = self.lua.create_registry_value(local_env)?;
    let internal = self.lua.create_registry_value(internal)?;
    Ok(Isolate {
      source,
      local_env,
      internal,
    })
  }

  pub async fn run_isolate<'lua, A: ToLuaMulti<'lua>, R: FromLuaMulti<'lua>>(
    &'lua self,
    isolate: &Isolate,
    chunk: &impl AsChunk<'lua>,
    name: &str,
    args: A,
  ) -> mlua::Result<R> {
    let env: Table = self.get_local_env(isolate)?;
    (self.lua.load(chunk))
      .set_environment(env)?
      .set_name(name)?
      .call_async::<A, R>(args)
      .await
  }

  pub async fn run_isolate_source<'lua, A: ToLuaMulti<'lua>, R: FromLuaMulti<'lua>>(
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

#[derive(Debug)]
pub struct Isolate {
  source: Source,
  local_env: RegistryKey,
  internal: RegistryKey,
}
