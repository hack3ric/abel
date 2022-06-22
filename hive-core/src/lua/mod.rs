pub(crate) mod context;
pub mod http;

mod byte_stream;
mod crypto;
mod env;
mod fs;
mod json;
mod print;
mod sandbox;
mod shared;

pub use fs::remove_service_local_storage;
pub use sandbox::Sandbox;

use crate::Result;
use futures::Future;
use mlua::{ExternalError, FromLua, Function, Lua, MultiValue, Table, ToLua, ToLuaMulti};
use std::sync::Arc;

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

#[derive(Debug, thiserror::Error)]
#[error("bad argument #{pos} to '{fn_name}' ({msg})")]
pub struct BadArgument {
  fn_name: &'static str,
  pos: u8,
  msg: Arc<dyn std::error::Error + Send + Sync>,
}

impl BadArgument {
  fn new(
    fn_name: &'static str,
    pos: u8,
    msg: impl Into<Box<dyn std::error::Error + Send + Sync>>,
  ) -> Self {
    let msg = msg.into().into();
    Self { fn_name, pos, msg }
  }
}

impl From<BadArgument> for mlua::Error {
  fn from(x: BadArgument) -> mlua::Error {
    x.to_lua_err()
  }
}

pub(super) fn extract_error<'lua, R, F>(lua: &'lua Lua, func: F) -> mlua::Result<MultiValue<'lua>>
where
  R: ToLuaMulti<'lua>,
  F: FnOnce() -> mlua::Result<R>,
{
  match func() {
    Ok(result) => lua.pack_multi(result),
    Err(error) => lua.pack_multi((mlua::Value::Nil, error.to_string())),
  }
}

pub(super) async fn extract_error_async<'lua, R, Fut>(
  lua: &'lua Lua,
  future: Fut,
) -> mlua::Result<MultiValue<'lua>>
where
  R: ToLuaMulti<'lua>,
  Fut: Future<Output = mlua::Result<R>>,
{
  match future.await {
    Ok(result) => lua.pack_multi(result),
    Err(error) => lua.pack_multi((mlua::Value::Nil, error.to_string())),
  }
}
