pub mod error;
pub mod global_env;
pub mod isolate;
pub mod require;
pub mod sandbox;

mod libs;
#[cfg(test)]
mod tests;

pub use libs::{fs, http, json, lua_std, rand, stream};

use crate::{Error, ErrorKind};
use error::{resolve_callback_error, CustomError};
use futures::Future;
use hyper::client::HttpConnector;
use hyper::Client;
use hyper_tls::HttpsConnector;
use mlua::{ExternalError, FromLua, FromLuaMulti, Function, Lua, Table, ToLua, ToLuaMulti};
use once_cell::sync::Lazy;
use std::sync::Arc;

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

#[derive(Debug)]
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
pub trait LuaCacheExt {
  fn create_cached_value<'lua, R, F>(&'lua self, key: &str, gen: F) -> mlua::Result<R>
  where
    R: FromLua<'lua> + ToLua<'lua> + Clone,
    F: FnOnce() -> mlua::Result<R>;

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
    F: FnOnce() -> mlua::Result<R>,
  {
    self.named_registry_value(key).or_else(|_| {
      let value = gen()?;
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
    self.create_cached_value(key, || self.create_function(f))
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
    self.create_cached_value(key, || self.create_async_function(f))
  }
}

pub fn sanitize_error(error: mlua::Error) -> Error {
  fn extract_custom_error(
    error: &Arc<dyn std::error::Error + Send + Sync + 'static>,
  ) -> Option<Error> {
    let maybe_custom = error.downcast_ref::<CustomError>();
    maybe_custom.map(|x| ErrorKind::Custom(x.clone()).into())
  }

  match error {
    mlua::Error::CallbackError { traceback, cause } => {
      let cause = resolve_callback_error(&cause);
      if let mlua::Error::ExternalError(error) = cause {
        if let Some(error) = extract_custom_error(error) {
          return error;
        }
      }
      format!("{cause}\n{traceback}").to_lua_err().into()
    }
    mlua::Error::ExternalError(error) => {
      extract_custom_error(&error).unwrap_or_else(|| mlua::Error::ExternalError(error).into())
    }
    _ => error.into(),
  }
}
