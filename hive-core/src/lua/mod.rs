pub mod http;

mod byte_stream;
mod context;
mod fs;
mod json;
mod permission;
mod sandbox;
mod env;

pub use sandbox::Sandbox;
pub use fs::remove_service_local_storage;

use crate::Result;
use mlua::{ExternalError, FromLua, Table};
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
    Self {
      fn_name,
      pos,
      msg: msg.into().into(),
    }
  }
}

impl From<BadArgument> for mlua::Error {
  fn from(x: BadArgument) -> mlua::Error {
    x.to_lua_err()
  }
}
