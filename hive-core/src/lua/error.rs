use crate::{Error, ErrorKind};
use futures::Future;
use mlua::{
  DebugNames, ExternalError, ExternalResult, FromLua, Function, Lua, LuaSerdeExt, MultiValue,
  ToLuaMulti, UserData,
};
use std::borrow::Cow;
use std::cell::{Ref, RefMut};
use std::sync::Arc;

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

pub async fn extract_error_async<'lua, R, Fut>(
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

pub fn sanitize_error(error: mlua::Error) -> Error {
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

fn resolve_callback_error(error: &mlua::Error) -> &mlua::Error {
  match error {
    mlua::Error::CallbackError {
      traceback: _,
      cause,
    } => resolve_callback_error(cause),
    _ => error,
  }
}

fn extract_custom_error(
  error: &Arc<dyn std::error::Error + Send + Sync + 'static>,
) -> Option<Error> {
  let maybe_custom = error.downcast_ref::<Error>().map(Error::kind);
  if let Some(ErrorKind::Custom {
    status,
    error,
    detail,
  }) = maybe_custom
  {
    Some(From::from(ErrorKind::Custom {
      status: *status,
      error: error.clone(),
      detail: detail.clone(),
    }))
  } else {
    None
  }
}

pub fn create_fn_error(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, error: mlua::Value| -> mlua::Result<()> {
    use mlua::Value::*;
    match error {
      Error(error) => Err(error),
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
        Err(lua_error(msg))
      }
    }
  })
}

// Error utilities

pub fn lua_error(s: impl Into<String>) -> mlua::Error {
  mlua::Error::RuntimeError(s.into())
}

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
  lua_error(arg_error_msg(lua, pos, msg, level))
}

pub fn tag_error(lua: &Lua, pos: usize, expected: &str, got: &str, level: usize) -> mlua::Error {
  arg_error(lua, pos, &format!("{expected} expected, got {got}"), level)
}

pub fn check_userdata<'lua, T: UserData + 'static>(
  lua: &'lua Lua,
  args: &'lua MultiValue<'lua>,
  pos: usize,
  expected: &str,
  level: usize,
) -> mlua::Result<Ref<'lua, T>> {
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
