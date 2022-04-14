use super::body::LuaBody;
use super::header_map::LuaHeaderMap;
use hyper::header::HeaderName;
use hyper::http::{HeaderMap, HeaderValue, StatusCode};
use hyper::{Body, Response};
use mlua::{
  ExternalError, ExternalResult, FromLua, Function, Lua, Table, UserData, UserDataFields,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
pub struct LuaResponse {
  pub status: StatusCode,
  pub headers: Rc<RefCell<HeaderMap>>,
  pub body: Option<LuaBody>,
}

impl LuaResponse {
  pub(crate) fn from_hyper(resp: Response<Body>) -> Self {
    let (parts, body) = resp.into_parts();
    Self {
      status: parts.status,
      headers: Rc::new(RefCell::new(parts.headers)),
      body: Some(body.into()),
    }
  }
}

impl UserData for LuaResponse {
  fn add_fields<'lua, F: UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_field_method_get("status", |_lua, this| Ok(this.status.as_u16()));
    fields.add_field_function_get("body", |lua, this| {
      let mut this_ = this.borrow_mut::<Self>()?;
      let body = this_.body.take();
      if let Some(body) = body {
        let x = lua.pack(body)?;
        this.set_named_user_value("body", x.clone())?;
        Ok(x)
      } else {
        this.get_named_user_value("body")
      }
    });
    fields.add_field_method_get("headers", |_lua, this| {
      Ok(LuaHeaderMap(this.headers.clone()))
    })
  }
}

impl<'lua> FromLua<'lua> for LuaResponse {
  fn from_lua(value: mlua::Value, lua: &Lua) -> mlua::Result<Self> {
    use mlua::Value::*;
    match value {
      x @ Table(_) | x @ Nil | x @ String(_) => {
        Ok(lua.unpack::<LuaBody>(x)?.into_default_response())
      }
      UserData(x) => {
        if let Ok(mut u) = x.take::<Self>() {
          if u.body.is_none() {
            let t = x.get_named_user_value::<_, LuaBody>("body")?;
            u.body = Some(t);
          }
          Ok(u)
        } else {
          Ok(lua.unpack::<LuaBody>(UserData(x))?.into_default_response())
        }
      }
      _ => Err("cannot convert to response".to_lua_err()),
    }
  }
}

impl From<LuaResponse> for Response<Body> {
  fn from(x: LuaResponse) -> Self {
    let headers = Rc::try_unwrap(x.headers)
      .map(RefCell::into_inner)
      .unwrap_or_else(|x| x.borrow().clone());

    let mut builder = Response::builder().status(x.status);
    *builder.headers_mut().unwrap() = headers;
    builder.body(x.body.unwrap().into()).unwrap()
  }
}

pub fn create_fn_create_response(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|_lua, params: Table| {
    let body = params.raw_get::<_, LuaBody>("body")?;
    let mut response = body.into_default_response();

    let status = params.raw_get::<_, Option<u16>>("status")?;
    if let Some(x) = status {
      response.status = StatusCode::from_u16(x)
        .map_err(|_| format!("invalid status code: {x}"))
        .to_lua_err()?;
    }

    let headers = params.raw_get::<_, Option<Table>>("headers")?;
    if let Some(x) = headers {
      let mut headers = response.headers.borrow_mut();
      for f in x.pairs::<String, String>() {
        let (k, v) = f?;
        headers.insert(
          HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| format!("invalid header value: {}", k))
            .to_lua_err()?,
          HeaderValue::from_str(&v)
            .map_err(|_| format!("invalid header value: {}", v))
            .to_lua_err()?,
        );
      }
    }

    Ok(response)
  })
}
