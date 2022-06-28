use crate::{Error, ErrorKind};
use futures::Future;
use mlua::{ExternalError, ExternalResult, Function, Lua, LuaSerdeExt, MultiValue, ToLuaMulti};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
#[error("bad argument #{pos} to '{fn_name}' ({msg})")]
pub struct BadArgument {
  fn_name: &'static str,
  pos: u8,
  msg: Arc<dyn std::error::Error + Send + Sync>,
}

impl BadArgument {
  pub fn new(
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
        Err(mlua::Error::RuntimeError(msg))
      }
    }
  })
}
