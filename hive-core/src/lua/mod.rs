mod response;
mod sandbox;

pub use sandbox::Sandbox;

use crate::Result;
use mlua::{FromLua, Table};

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
