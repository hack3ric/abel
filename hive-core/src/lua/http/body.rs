use crate::lua::byte_stream::ByteStream;
use crate::lua::context::Table;
use crate::Response;
use hyper::header::HeaderValue;
use hyper::{HeaderMap, StatusCode};
use mlua::{ExternalError, FromLua, Lua, LuaSerdeExt, ToLua};

pub enum Body {
  Empty,
  Json(serde_json::Value),
  Bytes(Vec<u8>),
  ByteStream(ByteStream),
}

impl Body {
  // pub fn from_hyper(body: hyper::Body)
  pub fn into_default_response(self) -> Response {
    let (status, headers) = match &self {
      Self::Empty => (StatusCode::NO_CONTENT, Default::default()),
      Self::Json(_) => {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        (StatusCode::OK, headers)
      }
      _ => Default::default(),
    };
    Response {
      status,
      headers,
      body: Some(self),
    }
  }
}

impl From<hyper::Body> for Body {
  fn from(body: hyper::Body) -> Self {
    Self::ByteStream(body.into())
  }
}

impl From<Body> for hyper::Body {
  fn from(body: Body) -> Self {
    match body {
      Body::Empty => hyper::Body::empty(),
      Body::Json(x) => x.to_string().into(),
      Body::Bytes(x) => x.into(),
      Body::ByteStream(x) => hyper::Body::wrap_stream(x.0),
    }
  }
}

impl<'lua> FromLua<'lua> for Body {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
    let result = match lua_value {
      mlua::Value::Nil => Self::Empty,
      x @ mlua::Value::Table(_) => Self::Json(lua.from_value(x)?),
      mlua::Value::String(s) => Self::Bytes(s.as_bytes().into()),
      mlua::Value::UserData(u) => {
        if let Ok(s) = u.take::<ByteStream>() {
          Self::ByteStream(s)
        } else if u.borrow::<Table>().is_ok() {
          Self::Json(lua.from_value(mlua::Value::UserData(u))?)
        } else {
          return Err("failed to turn object into body".to_lua_err());
        }
      }
      _ => return Err("failed to turn object into body".to_lua_err()),
    };
    Ok(result)
  }
}

impl<'lua> ToLua<'lua> for Body {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    match self {
      Self::Empty => Ok(mlua::Value::Nil),
      Self::Json(x) => lua.to_value(&x),
      Self::Bytes(x) => Ok(mlua::Value::String(lua.create_string(&x)?)),
      Self::ByteStream(x) => lua.pack(x),
    }
  }
}
