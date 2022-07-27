pub mod byte_stream;
pub mod crypto;
pub mod error;
pub mod fs;
pub mod global_env;
pub mod http;
pub mod isolate;
pub mod json;
pub mod lua_std;
pub mod sandbox;

mod spawn;
#[cfg(test)]
mod tests;

use futures::Future;
use hyper::client::HttpConnector;
use hyper::Client;
use hyper_tls::HttpsConnector;
use log::info;
use mlua::{
  ExternalError, ExternalResult, FromLua, FromLuaMulti, Function, Lua, MultiValue, Table, ToLua,
  ToLuaMulti,
};
use once_cell::sync::Lazy;

static LUA_HTTP_CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

pub trait LuaTableExt<'a> {
  fn raw_get_path<T: FromLua<'a>>(&self, base: &str, path: &[&str]) -> mlua::Result<T>;
}

impl<'a> LuaTableExt<'a> for Table<'a> {
  fn raw_get_path<T: FromLua<'a>>(&self, base: &str, path: &[&str]) -> mlua::Result<T> {
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

enum LuaEither<T, U> {
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

/// Supports caching values in Lua state's registry.
///
/// Note that only values that does not rely on other arguments (say, a
/// closure that captures nothing) should be cached. A counterexample is
/// `fs.open`, which captures `source` and `local_storage_path` as argument.
trait LuaCacheExt {
  fn create_cached_value<'lua, R, F>(&'lua self, key: &str, gen: F) -> mlua::Result<R>
  where
    R: FromLua<'lua> + ToLua<'lua> + Clone,
    F: FnOnce(&'lua Lua) -> mlua::Result<R>;

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
  fn create_cached_value<'lua, R, F>(&'lua self, key: &str, gen: F) -> mlua::Result<R>
  where
    R: FromLua<'lua> + ToLua<'lua> + Clone,
    F: FnOnce(&'lua Lua) -> mlua::Result<R>,
  {
    self.named_registry_value(key).or_else(|_| {
      let value = gen(self)?;
      self.set_named_registry_value(key, value.clone())?;
      Ok(value)
    })
  }

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
    self.create_cached_value(key, |lua| lua.create_function(f))
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
    self.create_cached_value(key, |lua| lua.create_async_function(f))
  }
}

fn create_fn_print_to_log<'a>(lua: &'a Lua, service_name: &str) -> mlua::Result<Function<'a>> {
  let tostring: Function = lua.globals().raw_get("tostring")?;
  let target = format!("service '{service_name}'");
  let f = lua.create_function(move |_lua, (tostring, args): (Function, MultiValue)| {
    let s = args
      .into_iter()
      .try_fold(String::new(), |mut init, x| -> mlua::Result<_> {
        let string = tostring.call::<_, mlua::String>(x)?;
        let string = std::str::from_utf8(string.as_bytes()).to_lua_err()?;
        init.push_str(string);
        (0..8 - string.as_bytes().len() % 8).for_each(|_| init.push(' '));
        Ok(init)
      })?;
    info!(target: &target, "{s}");
    Ok(())
  })?;
  f.bind(tostring)
}
