use crate::ErrorKind;
use mlua::{
  DebugNames, ExternalError, ExternalResult, FromLua, Function, Lua, LuaSerdeExt, MultiValue,
  Table, UserData,
};
use std::borrow::Cow;
use std::cell::{Ref, RefMut};

pub fn create_fn_error(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, error: mlua::Value| -> mlua::Result<()> {
    use mlua::Value::*;
    match error {
      Table(custom_error) => {
        let status = custom_error
          .raw_get::<_, u16>("status")?
          .try_into()
          .to_lua_err()?;
        let error_str = custom_error.raw_get::<_, mlua::String>("error")?;
        let error = std::str::from_utf8(error_str.as_bytes())
          .to_lua_err()?
          .into();
        let detail = custom_error.raw_get::<_, mlua::Value>("detail")?;
        let result = ErrorKind::Custom {
          status,
          error,
          detail: lua.from_value(detail)?,
        };
        Err(crate::Error::from(result).to_lua_err())
      }
      _ => {
        let type_name = error.type_name();
        let msg = if let Some(x) = lua.coerce_string(error)? {
          x.to_string_lossy().into_owned()
        } else {
          format!("(error object is a {type_name} value)")
        };
        Err(rt_error(msg))
      }
    }
  })
}

// Error utilities

pub fn rt_error(s: impl ToString) -> mlua::Error {
  mlua::Error::RuntimeError(s.to_string())
}

macro_rules! rt_error_fmt {
  ($($args:tt)*) => {
    $crate::lua::error::rt_error(format!($($args)*))
  };
}

pub(crate) use rt_error_fmt;

fn arg_error_msg(lua: &Lua, mut pos: usize, msg: &str, level: usize) -> String {
  if let Some(d) = lua.inspect_stack(level) {
    let DebugNames { name, name_what } = d.names();
    let name = name
      .map(String::from_utf8_lossy)
      .unwrap_or(Cow::Borrowed("?"));
    if name_what == Some(b"method") {
      pos -= 1;
      if pos == 0 {
        return format!("calling '{name}' on bad self ({msg})");
      }
    }
    format!("bad argument #{pos} to '{name}' ({msg})")
  } else {
    format!("bad argument #{pos} ({msg})")
  }
}

pub fn arg_error(lua: &Lua, pos: usize, msg: &str, level: usize) -> mlua::Error {
  rt_error(arg_error_msg(lua, pos, msg, level))
}

pub fn tag_error(lua: &Lua, pos: usize, expected: &str, got: &str, level: usize) -> mlua::Error {
  arg_error(lua, pos, &format!("{expected} expected, got {got}"), level)
}

pub fn check_userdata<'a, 'lua, T: UserData + 'static>(
  lua: &'lua Lua,
  args: &'a MultiValue<'lua>,
  pos: usize,
  expected: &str,
  level: usize,
) -> mlua::Result<Ref<'a, T>> {
  match args.iter().nth(pos - 1) {
    Some(mlua::Value::UserData(u)) => u
      .borrow::<T>()
      .map_err(|_| tag_error(lua, pos, expected, "other userdata", level)),
    Some(other) => Err(tag_error(lua, pos, expected, other.type_name(), level)),
    None => Err(tag_error(lua, pos, expected, "no value", level)),
  }
}

pub fn check_userdata_mut<'lua, T: UserData + 'static>(
  lua: &'lua Lua,
  args: &'lua MultiValue<'lua>,
  pos: usize,
  expected: &str,
  level: usize,
) -> mlua::Result<RefMut<'lua, T>> {
  match args.iter().nth(pos - 1) {
    Some(mlua::Value::UserData(u)) => u
      .borrow_mut::<T>()
      .map_err(|_| tag_error(lua, pos, expected, "other userdata", level)),
    Some(other) => Err(tag_error(lua, pos, expected, other.type_name(), level)),
    None => Err(tag_error(lua, pos, expected, "no value", level)),
  }
}

pub fn check_arg<'lua, T: FromLua<'lua>>(
  lua: &'lua Lua,
  args: &MultiValue<'lua>,
  pos: usize,
  expected: &str,
  level: usize,
) -> mlua::Result<T> {
  args
    .iter()
    .nth(pos - 1)
    .map(|value| {
      lua
        .unpack(value.clone())
        .map_err(|_| tag_error(lua, pos, expected, value.type_name(), level))
    })
    .unwrap_or_else(|| Err(tag_error(lua, pos, expected, "no value", level)))
}

pub trait TableCheckExt<'lua> {
  fn check_raw_get<T: FromLua<'lua>>(
    &self,
    lua: &'lua Lua,
    field: &str,
    expected: &str,
  ) -> mlua::Result<T>;
}

impl<'lua> TableCheckExt<'lua> for Table<'lua> {
  fn check_raw_get<T: FromLua<'lua>>(
    &self,
    lua: &'lua Lua,
    field: &str,
    expected: &str,
  ) -> mlua::Result<T> {
    let val: mlua::Value = self.raw_get(field)?;
    let type_name = val.type_name();
    lua
      .unpack(val)
      .map_err(|_| rt_error_fmt!("bad field '{field}' ({expected} expected, got {type_name})"))
  }
}
