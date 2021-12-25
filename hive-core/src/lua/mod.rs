mod sandbox;

pub use sandbox::Sandbox;

use crate::Result;
use mlua::{FromLua, Table};

pub trait LuaTableExt<'a> {
  fn raw_get_path<T: FromLua<'a>>(&self, base: &str, path: &[&str]) -> Result<T>;
}

fn raw_get_path<'a, T: FromLua<'a>>(
  table: &Table<'a>,
  base: &str,
  path: &[&str],
) -> mlua::Result<T> {
  if path.len() == 1 {
    Ok(table.raw_get(path[0])?)
  } else {
    raw_get_path(
      &table.raw_get::<_, Table>(path[0])?,
      &(base.to_string() + "." + path[0]),
      &path[1..],
    )
  }
}

impl<'a> LuaTableExt<'a> for Table<'a> {
  fn raw_get_path<T: FromLua<'a>>(&self, base: &str, path: &[&str]) -> Result<T> {
    let result = raw_get_path(self, base, path).map_err(|mut error| {
      if let mlua::Error::FromLuaConversionError { message, .. } = &mut error {
        *message = Some(base.to_string());
      }
      error
    })?;
    Ok(result)
  }
}
