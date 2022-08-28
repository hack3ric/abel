use super::http::{create_fn_http_create_uri, create_fn_http_request};
use super::LuaCacheExt;
use crate::lua::LuaTableExt;
use crate::source::{Source, SourceUserData};
use mlua::{Function, Lua, RegistryKey, Table, TableExt};

#[derive(Debug)]
pub struct Isolate {
  pub(crate) source: Source,
  pub(crate) local_env: RegistryKey,
  pub(crate) internal: RegistryKey,
}

pub struct IsolateBuilder<'lua> {
  lua: &'lua Lua,
  source: Source,
  local_env: Table<'lua>,
  internal: Table<'lua>,
  preload: Table<'lua>,
}

impl<'lua> IsolateBuilder<'lua> {
  pub(super) fn new(lua: &'lua Lua, source: Source) -> mlua::Result<Self> {
    let (local_env, internal): (_, Table) = isolate_bootstrap(lua, source.clone())?;
    let preload = internal.raw_get_path("<internal>", &["package", "preload"])?;
    Ok(Self {
      lua,
      source,
      local_env,
      internal,
      preload,
    })
  }

  pub fn add_side_effect(
    self,
    f: impl FnOnce(&Lua, Table, Table) -> mlua::Result<()>,
  ) -> mlua::Result<Self> {
    f(self.lua, self.local_env.clone(), self.internal.clone())?;
    Ok(self)
  }

  // TODO: How to access local_env in loaders?
  pub fn add_lib(
    self,
    name: &str,
    f: impl FnOnce(&Lua) -> mlua::Result<Function>,
  ) -> mlua::Result<Self> {
    self.preload.raw_set(name, f(self.lua)?)?;
    Ok(self)
  }

  pub fn add_lua_lib(self, name: &str, code: &str) -> mlua::Result<Self> {
    let key = format!("abel:lua_preload_{name}");
    let preload = self.lua.create_cached_value(&key, || {
      (self.lua)
        .load(code)
        .set_name(&key)?
        .set_environment(self.local_env.clone())?
        .into_function()
    })?;
    self.preload.raw_set(name, preload)?;
    Ok(self)
  }

  pub fn load_libs<'a>(self, names: impl IntoIterator<Item = &'a str>) -> mlua::Result<Self> {
    for name in names {
      let lib: mlua::Value = self.local_env.call_function("require", name)?;
      self.local_env.raw_set(name, lib)?;
    }
    Ok(self)
  }

  pub fn build(self) -> mlua::Result<Isolate> {
    let local_env = self.lua.create_registry_value(self.local_env)?;
    let internal = self.lua.create_registry_value(self.internal)?;
    Ok(Isolate {
      source: self.source,
      local_env,
      internal,
    })
  }
}

fn isolate_bootstrap(lua: &Lua, source: Source) -> mlua::Result<(Table, Table)> {
  let bootstrap = lua.create_cached_value("abel:isolate_bootstrap", || {
    lua
      .load(include_str!("isolate_bootstrap.lua"))
      .set_name("@[isolate_bootstrap]")?
      .into_function()
  })?;
  bootstrap.call((
    SourceUserData(source),
    create_fn_http_request(lua)?,
    create_fn_http_create_uri(lua)?,
  ))
}
