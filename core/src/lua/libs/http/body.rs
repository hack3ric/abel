use super::LuaResponse;
use crate::lua::abel::{abel_spawn, create_fn_spawn};
use crate::lua::error::rt_error;
use crate::lua::fs::LuaFile;
use crate::lua::stream::{is_stream, ByteStream};
use crate::lua::LuaCacheExt;
use hyper::body::Bytes;
use hyper::header::HeaderValue;
use hyper::{Body, HeaderMap, StatusCode};
use mlua::{AnyUserData, Lua, LuaSerdeExt, ToLua, UserData};
use std::cell::RefCell;
use std::rc::Rc;

pub enum LuaBody {
  Empty,
  Json(serde_json::Value),
  Bytes(Vec<u8>),
  Stream(Body),
}

impl LuaBody {
  pub fn into_default_response(self) -> LuaResponse {
    let (status, headers) = match &self {
      Self::Empty => (StatusCode::NO_CONTENT, Default::default()),
      Self::Json(_) => {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        (StatusCode::OK, headers)
      }
      _ => Default::default(),
    };
    LuaResponse {
      status,
      headers: Rc::new(RefCell::new(headers)),
      body: Some(self),
    }
  }

  pub(crate) fn from_value<'a>(
    lua: &'a Lua,
    value: mlua::Value<'a>,
  ) -> mlua::Result<Result<Self, String>> {
    let result = match value {
      mlua::Value::Nil => Ok(Self::Empty),
      mlua::Value::String(s) => Ok(Self::Bytes(s.as_bytes().into())),

      // Optimization for native stream
      mlua::Value::UserData(u) if u.is::<ByteStream>() => u
        .take::<ByteStream>()
        .map(|x| Ok(Self::Stream(Body::wrap_stream(x.0))))?,
      // Optimization for file
      mlua::Value::UserData(u) if u.is::<LuaFile>() => u.take::<LuaFile>().map(|x| {
        Ok(Self::Stream(Body::wrap_stream(
          ByteStream::from_async_read(x.0).0,
        )))
      })?,
      _ if is_stream(lua, value.clone())? => body_from_lua_stream(lua, value).map(Ok)?,
      mlua::Value::UserData(_) => Err("stream expected, got other userdata".into()),

      x @ mlua::Value::Table(_) => serde_json::to_value(&x)
        .map(Self::Json)
        .map_err(|x| x.to_string()),
      _ => Err(format!(
        "string, JSON table or stream expected, got {}",
        value.type_name()
      )),
    };
    Ok(result)
  }
}

fn body_from_lua_stream(lua: &Lua, stream: mlua::Value) -> mlua::Result<LuaBody> {
  struct LuaBodySender(hyper::body::Sender);

  impl UserData for LuaBodySender {
    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
      methods.add_meta_function("__close", |_lua, this: AnyUserData| {
        let _ = this.take::<Self>();
        Ok(())
      });

      #[allow(clippy::await_holding_refcell_ref)]
      methods.add_async_function(
        "send",
        |_lua, (this, data): (AnyUserData, mlua::String)| async move {
          let mut tx = this.borrow_mut::<Self>()?;
          tx.0
            .send_data(Bytes::copy_from_slice(data.as_bytes()))
            .await
            .map_err(rt_error)
        },
      );
    }
  }

  let (tx, body) = Body::channel();
  let f = lua
    .create_cached_value("abel:body_spawn_send", || {
      const SRC: &str = r#"
        local st, tx <close>, spawn = ...
        local p
        while true do
          local bytes = st:read()
          if p then p:await() end
          if not bytes then break end
          p = spawn(tx.send, tx, bytes)
        end
      "#;
      lua.load(SRC).into_function()
    })?
    .bind((stream, LuaBodySender(tx), create_fn_spawn(lua)?))?;
  let _ = abel_spawn(lua, f)?;

  Ok(LuaBody::Stream(body))
}

impl From<Body> for LuaBody {
  fn from(body: Body) -> Self {
    Self::Stream(body)
  }
}

impl From<LuaBody> for Body {
  fn from(body: LuaBody) -> Self {
    match body {
      LuaBody::Empty => Body::empty(),
      LuaBody::Json(x) => x.to_string().into(),
      LuaBody::Bytes(x) => x.into(),
      LuaBody::Stream(x) => x,
    }
  }
}

impl<'lua> ToLua<'lua> for LuaBody {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    match self {
      Self::Empty => Ok(mlua::Value::Nil),
      Self::Json(x) => lua.to_value(&x),
      Self::Bytes(x) => Ok(mlua::Value::String(lua.create_string(&x)?)),
      Self::Stream(x) => lua.pack(ByteStream::from(x)),
    }
  }
}
