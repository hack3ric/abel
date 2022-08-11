use crate::task::TimeoutError;
use bstr::ByteSlice;
use hyper::StatusCode;
use mlua::Error::*;
use mlua::Value::Nil;
use mlua::{
  AnyUserData, DebugNames, DebugSource, ExternalError, FromLua, Function, Lua, LuaSerdeExt,
  MultiValue, RegistryKey, Table, UserData,
};
use ouroboros::self_referencing;
use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::fmt::Display;

#[derive(Debug)]
pub struct CustomError {
  pub status: StatusCode,
  pub error: String,
  pub detail: serde_json::Value,
  source: Option<RegistryKey>,
}

impl Display for CustomError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if self.detail.is_null() {
      write!(f, "({}) {}", self.status, self.error)
    } else {
      write!(f, "({}) {} {:?}", self.status, self.error, self.detail)
    }
  }
}

impl std::error::Error for CustomError {}

impl Clone for CustomError {
  fn clone(&self) -> Self {
    Self {
      status: self.status,
      error: self.error.clone(),
      detail: self.detail.clone(),
      source: None,
    }
  }
}

pub fn resolve_callback_error(error: &mlua::Error) -> &mlua::Error {
  match error {
    mlua::Error::CallbackError {
      traceback: _,
      cause,
    } => resolve_callback_error(cause),
    _ => error,
  }
}

pub fn modify_global_error_handling(lua: &Lua) -> mlua::Result<()> {
  let handle_http_error = create_fn_handle_http_error(lua)?;
  let pcall = create_fn_pcall(lua)?;
  lua
    .load(include_str!("error.lua"))
    .set_name("@[error]")?
    .call((handle_http_error, pcall))
}

fn create_fn_handle_http_error(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, custom_error: Table| -> mlua::Result<()> {
    let result = CustomError {
      status: custom_error
        .check_raw_get::<Option<u16>>(lua, "status", "u16")?
        .unwrap_or(500)
        .try_into()
        .map_err(|_| bad_field("status", "invalid status code"))?,
      error: custom_error
        .check_raw_get::<Option<mlua::String>>(lua, "error", "string")?
        .map(|x| mlua::Result::Ok(x.to_str()?.into()).map_err(|error| bad_field("error", error)))
        .transpose()?
        .unwrap_or_else(|| "".into()),
      detail: custom_error
        .raw_get::<_, mlua::Value>("detail")
        .and_then(|x| lua.from_value(x))
        .map_err(|error| bad_field("detail", error))?,
      source: Some(lua.create_registry_value(custom_error)?),
    };
    Err(result.to_lua_err())
  })
}

fn create_fn_pcall(lua: &Lua) -> mlua::Result<Function> {
  lua.create_async_function(|lua, args: MultiValue| async move {
    let (success, value): (bool, mlua::Value) = lua
      .named_registry_value::<_, Function>("lua_pcall")?
      .call_async(args)
      .await?;
    if success {
      Ok((true, value))
    } else {
      let value = if let mlua::Value::Error(error) = value {
        if let mlua::Error::ExternalError(ext) = resolve_callback_error(&error) {
          if ext.is::<TimeoutError>() {
            return Err(error);
          }
          ext
            .downcast_ref::<CustomError>()
            .and_then(|x| x.source.as_ref())
            .map(|x| lua.registry_value(x))
            .transpose()?
            .map(Ok)
            .unwrap_or_else(|| lua.pack(get_error_msg(error)))?
        } else {
          lua.pack(get_error_msg(error))?
        }
      } else {
        value
      };
      Ok((false, value))
    }
  })
}

fn get_error_msg(error: mlua::Error) -> String {
  match error {
    SyntaxError { message, .. } => message,
    RuntimeError(e) | MemoryError(e) => e,
    CallbackError { cause, .. } => get_error_msg(resolve_callback_error(&cause).clone()),
    _ => error.to_string(),
  }
}

// TODO: pub fn create_fn_xpcall

// Error utilities

pub fn rt_error(s: impl ToString) -> mlua::Error {
  mlua::Error::RuntimeError(s.to_string())
}

#[macro_export]
macro_rules! rt_error_fmt {
  ($($args:tt)*) => {
    $crate::lua::error::rt_error(format!($($args)*))
  };
}
pub use rt_error_fmt;

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

    let prefix = lua
      .inspect_stack(level + 1)
      .and_then(|d| {
        let DebugSource { short_src, .. } = d.source();
        let line = d.curr_line();
        if line > 0 {
          short_src.map(|x| Cow::Owned(format!("{}:{line}: ", x.as_bstr())))
        } else {
          None
        }
      })
      .unwrap_or(Cow::Borrowed(""));

    if name_what == Some(b"method") {
      pos -= 1;
      if pos == 0 {
        return format!("{prefix}calling '{name}' on bad self ({msg})");
      }
    }
    format!("{prefix}bad argument #{pos} to '{name}' ({msg})")
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
