pub(crate) mod context;
pub mod http;

mod byte_stream;
mod crypto;
mod error;
mod fs;
mod global_env;
mod isolate;
mod json;
mod lua_std;
mod print;
mod runtime;
mod sandbox;

#[cfg(test)]
mod tests;

pub use isolate::Isolate;
pub use runtime::Runtime;

use crate::Result;
use futures::Future;
use mlua::{ExternalError, FromLua, FromLuaMulti, Function, Lua, Table, ToLuaMulti};

pub trait LuaTableExt<'a> {
  fn raw_get_path<T: FromLua<'a>>(&self, base: &str, path: &[&str]) -> Result<T>;
}

fn raw_get_path<'a, T: FromLua<'a>>(
  table: &Table<'a>,
  base: &mut String,
  path: &[&str],
) -> mlua::Result<T> {
  base.extend([".", path[0]]);
  if path.len() == 1 {
    Ok(table.raw_get(path[0])?)
  } else {
    raw_get_path(&table.raw_get::<_, Table>(path[0])?, base, &path[1..])
  }
}

impl<'a> LuaTableExt<'a> for Table<'a> {
  fn raw_get_path<T: FromLua<'a>>(&self, base: &str, path: &[&str]) -> Result<T> {
    let mut base = base.into();
    let result = raw_get_path(self, &mut base, path).map_err(|mut error| {
      if let mlua::Error::FromLuaConversionError { message, .. } = &mut error {
        *message = Some(base);
      }
      error
    })?;
    Ok(result)
  }
}

pub enum LuaEither<T, U> {
  Left(T),
  Right(U),
}

impl<'lua, T: FromLua<'lua>, U: FromLua<'lua>> FromLua<'lua> for LuaEither<T, U> {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
    lua
      .unpack::<T>(lua_value.clone())
      .map(Self::Left)
      .or_else(|_| lua.unpack::<U>(lua_value).map(Self::Right))
      .map_err(|_| "failed to convert Lua value to LuaEither".to_lua_err())
  }
}

trait LuaCacheExt {
  fn create_cached_function<'lua, A, R, F>(
    &'lua self,
    key: &str,
    f: F,
  ) -> mlua::Result<Function<'lua>>
  where
    A: FromLuaMulti<'lua>,
    R: ToLuaMulti<'lua>,
    F: Fn(&'lua Lua, A) -> mlua::Result<R> + 'static;

  fn create_cached_async_function<'lua, A, R, F, Fut>(
    &'lua self,
    key: &str,
    f: F,
  ) -> mlua::Result<Function<'lua>>
  where
    A: FromLuaMulti<'lua>,
    R: ToLuaMulti<'lua>,
    F: Fn(&'lua Lua, A) -> Fut + 'static,
    Fut: Future<Output = mlua::Result<R>> + 'lua;
}

impl LuaCacheExt for Lua {
  fn create_cached_function<'lua, A, R, F>(
    &'lua self,
    key: &str,
    f: F,
  ) -> mlua::Result<Function<'lua>>
  where
    A: FromLuaMulti<'lua>,
    R: ToLuaMulti<'lua>,
    F: Fn(&'lua Lua, A) -> mlua::Result<R> + 'static,
  {
    self.named_registry_value(key).or_else(|_| {
      let f = self.create_function(f)?;
      self.set_named_registry_value(key, f.clone())?;
      Ok(f)
    })
  }

  fn create_cached_async_function<'lua, A, R, F, Fut>(
    &'lua self,
    key: &str,
    f: F,
  ) -> mlua::Result<Function<'lua>>
  where
    A: FromLuaMulti<'lua>,
    R: ToLuaMulti<'lua>,
    F: Fn(&'lua Lua, A) -> Fut + 'static,
    Fut: Future<Output = mlua::Result<R>> + 'lua,
  {
    self.named_registry_value(key).or_else(|_| {
      let f = self.create_async_function(f)?;
      self.set_named_registry_value(key, f.clone())?;
      Ok(f)
    })
  }
}

pub fn create_preload_routing(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| {
    let module = lua.create_table()?;
    for f in lua
      .globals()
      .raw_get::<_, Table>("routing")?
      .pairs::<mlua::Value, mlua::Value>()
    {
      let (k, v) = f?;
      module.raw_set(k, v)?;
    }
    Ok(module)
  })
}
