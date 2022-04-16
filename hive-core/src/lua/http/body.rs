use crate::lua::byte_stream::ByteStream;
use crate::lua::shared::SharedTable;
use crate::LuaResponse;
use hyper::header::HeaderValue;
use hyper::{Body, HeaderMap, StatusCode};
use mlua::{ExternalError, ExternalResult, FromLua, Lua, LuaSerdeExt, ToLua};
use std::cell::RefCell;
use std::rc::Rc;

pub enum LuaBody {
  Empty,
  Json(serde_json::Value),
  Bytes(Vec<u8>),
  ByteStream(ByteStream),
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
}

impl From<Body> for LuaBody {
  fn from(body: Body) -> Self {
    Self::ByteStream(body.into())
  }
}

impl From<LuaBody> for Body {
  fn from(body: LuaBody) -> Self {
    match body {
      LuaBody::Empty => Body::empty(),
      LuaBody::Json(x) => x.to_string().into(),
      LuaBody::Bytes(x) => x.into(),
      LuaBody::ByteStream(x) => Body::wrap_stream(x.0),
    }
  }
}

impl<'lua> FromLua<'lua> for LuaBody {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    let result = match lua_value {
      mlua::Value::Nil => Self::Empty,
      x @ mlua::Value::Table(_) => Self::Json(serde_json::to_value(&x).to_lua_err()?),
      mlua::Value::String(s) => Self::Bytes(s.as_bytes().into()),
      mlua::Value::UserData(u) => {
        if let Ok(s) = u.take::<ByteStream>() {
          Self::ByteStream(s)
        } else if let Ok(x) = u.borrow::<SharedTable>() {
          Self::Json(serde_json::to_value(&*x).to_lua_err()?)
        } else {
          return Err("failed to turn object into body".to_lua_err());
        }
      }
      _ => return Err("failed to turn object into body".to_lua_err()),
    };
    Ok(result)
  }
}

impl<'lua> ToLua<'lua> for LuaBody {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    match self {
      Self::Empty => Ok(mlua::Value::Nil),
      Self::Json(x) => lua.to_value(&x),
      Self::Bytes(x) => Ok(mlua::Value::String(lua.create_string(&x)?)),
      Self::ByteStream(x) => lua.pack(x),
    }
  }
}
