use crate::ErrorKind;
use mlua::Value::Nil;
use mlua::{
  AnyUserData, DebugNames, ExternalError, FromLua, Function, Lua, LuaSerdeExt, MultiValue, Table,
  TableExt, UserData,
};
use ouroboros::self_referencing;
use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::fmt::Display;

pub(crate) fn resolve_callback_error(error: &mlua::Error) -> &mlua::Error {
  match error {
    mlua::Error::CallbackError {
      traceback: _,
      cause,
    } => resolve_callback_error(cause),
    _ => error,
  }
}

fn error_fn(lua: &Lua, error: mlua::Value) -> mlua::Result<()> {
  use mlua::Value::*;
  match error {
    Table(custom_error) => {
      let status = custom_error
        .check_raw_get::<u16>(lua, "status", "u16")
        .map_err(rt_error)?
        .try_into()
        .map_err(|_| bad_field("status", "invalid status code"))?;
      let error = custom_error.check_raw_get::<mlua::String>(lua, "error", "string")?;
      let error = std::str::from_utf8(error.as_bytes())
        .map_err(|error| bad_field("error", error))?
        .into();
      let detail = custom_error.raw_get::<_, mlua::Value>("detail")?;
      let detail = lua
        .from_value(detail)
        .map_err(|error| bad_field("detail", error))?;
      let result = ErrorKind::Custom {
        status,
        error,
        detail,
      };
      Err(crate::Error::from(result).to_lua_err())
    }
    Error(error) => Err(resolve_callback_error(&error).clone()),
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
}

pub fn create_fn_error(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(error_fn)
}

pub fn create_fn_assert(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, mut args: MultiValue| {
    let pred = args
      .pop_front()
      .ok_or_else(|| arg_error(lua, 1, "value expected", 0))?;
    let error = args
      .pop_front()
      .map(Ok)
      .unwrap_or_else(|| lua.pack("assertion failed!"))?;
    if let mlua::Value::Boolean(false) | mlua::Value::Nil = pred {
      error_fn(lua, error).map(|_| unreachable!())
    } else {
      Ok(pred)
    }
  })
}

pub fn get_error_msg(error: mlua::Error) -> String {
  use mlua::Error::*;
  match error {
    SyntaxError { message, .. } => message,
    RuntimeError(e) | MemoryError(e) => e,
    CallbackError { cause, .. } => get_error_msg(resolve_callback_error(&cause).clone()),
    _ => error.to_string(),
  }
}

pub fn create_fn_pcall(lua: &Lua) -> mlua::Result<Function> {
  lua.create_async_function(|lua, mut args: MultiValue| async move {
    let f = args
      .pop_front()
      .ok_or_else(|| arg_error(lua, 1, "value expected", 1))?;
    let result = match f {
      mlua::Value::Function(f) => f
        .call_async::<_, MultiValue>(args)
        .await
        .map_err(get_error_msg),
      mlua::Value::Table(f) => f.call_async(args).await.map_err(get_error_msg),
      _ => Err(format!("attempt to call a {} value", f.type_name())),
    };

    match result {
      Ok(result) => Ok((true, result)),
      Err(error) => Ok((false, lua.pack_multi(error)?)),
    }
  })
}

// TODO: pub fn create_fn_xpcall

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

// Note on `level`:
//
// Tells Lua how deep it should dig through stack trace to find the function's
// name.
//
// Initial: 0; async +1; `Function::bind` +1

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

pub fn tag_handler(
  lua: &Lua,
  pos: usize,
  level: usize,
) -> impl Fn((&'static str, &'static str)) -> mlua::Error + '_ {
  move |(expected, got)| tag_error(lua, pos, expected, got, level)
}

pub fn bad_field(field: &str, msg: impl Display) -> mlua::Error {
  rt_error_fmt!("bad field '{field}' ({msg})")
}

pub fn check_value<'lua, T: FromLua<'lua>>(
  lua: &'lua Lua,
  value: Option<mlua::Value<'lua>>,
  expected: &'static str,
) -> Result<T, (&'static str, &'static str)> {
  if let Some(value) = value {
    let type_name = value.type_name();
    lua.unpack(value).map_err(|_| (expected, type_name))
  } else {
    Err((expected, "no value"))
  }
}

pub fn check_integer(value: Option<mlua::Value>) -> Result<i64, (&'static str, &'static str)> {
  match value {
    Some(mlua::Value::Integer(i)) => Ok(i),
    Some(mlua::Value::Number(_)) => Err("float"),
    Some(value) => Err(value.type_name()),
    None => Err("no value"),
  }
  .map_err(|got| ("integer", got))
}

pub fn check_string<'lua>(
  lua: &'lua Lua,
  value: Option<mlua::Value<'lua>>,
) -> Result<mlua::String<'lua>, (&'static str, &'static str)> {
  check_value(lua, value, "string")
}

pub fn check_truthiness(value: Option<mlua::Value>) -> bool {
  match value {
    Some(mlua::Value::Boolean(b)) => b,
    Some(Nil) | None => false,
    Some(_) => true,
  }
}

#[self_referencing]
pub struct UserDataRef<'lua, T: UserData + 'static> {
  pub userdata: AnyUserData<'lua>,
  #[borrows(userdata)]
  #[covariant]
  pub borrowed: Ref<'this, T>,
}

impl<'lua, T: UserData + 'static> UserDataRef<'lua, T> {
  pub fn into_any(self) -> AnyUserData<'lua> {
    self.into_heads().userdata
  }
}

pub fn check_userdata<'lua, T: UserData + 'static>(
  value: Option<mlua::Value<'lua>>,
  expected: &'static str,
) -> Result<UserDataRef<'lua, T>, (&'static str, &'static str)> {
  match value {
    Some(mlua::Value::UserData(userdata)) => UserDataRefTryBuilder {
      userdata,
      borrowed_builder: |u| u.borrow::<T>().map_err(|_| "other userdata"),
    }
    .try_build(),
    Some(value) => Err(value.type_name()),
    None => Err("no value"),
  }
  .map_err(|got| (expected, got))
}

#[self_referencing]
pub struct UserDataRefMut<'lua, T: UserData + 'static> {
  pub userdata: AnyUserData<'lua>,
  #[borrows(mut userdata)]
  #[covariant]
  pub borrowed: RefMut<'this, T>,
}

impl<'lua, T: UserData + 'static> UserDataRefMut<'lua, T> {
  pub fn into_any(self) -> AnyUserData<'lua> {
    self.into_heads().userdata
  }
}

pub fn check_userdata_mut<'lua, T: UserData + 'static>(
  value: Option<mlua::Value<'lua>>,
  expected: &'static str,
) -> Result<UserDataRefMut<'lua, T>, (&'static str, &'static str)> {
  match value {
    Some(mlua::Value::UserData(userdata)) => UserDataRefMutTryBuilder {
      userdata,
      borrowed_builder: |u| u.borrow_mut::<T>().map_err(|_| "other userdata"),
    }
    .try_build(),
    Some(value) => Err(value.type_name()),
    None => Err("no value"),
  }
  .map_err(|got| (expected, got))
}

// #[deprecated]
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
      .map_err(|_| bad_field(field, format!("{expected} expected, got {type_name}")))
  }
}
