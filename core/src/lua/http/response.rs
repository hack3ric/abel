use super::body::LuaBody;
use super::check_headers;
use super::header_map::LuaHeaderMap;
use crate::lua::error::{bad_field, check_value, rt_error_fmt, tag_handler, TableCheckExt};
use crate::lua::LuaCacheExt;
use hyper::http::{HeaderMap, StatusCode};
use hyper::{Body, Response};
use mlua::{FromLua, Function, Lua, MultiValue, Table, UserData, UserDataFields};
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
      x @ Table(_) | x @ Nil | x @ String(_) => Ok(
        LuaBody::from_value(lua, x)?
          .map_err(|error| rt_error_fmt!("failed to read body ({error})"))?
          .into_default_response(),
      ),
      UserData(x) => {
        if let Ok(mut u) = x.take::<Self>() {
          if u.body.is_none() {
            let t = x.get_named_user_value::<_, mlua::Value>("body")?;
            let body = LuaBody::from_value(lua, t)?
              .map_err(|error| rt_error_fmt!("failed to get body from response ({error})"))?;
            u.body = Some(body);
          }
          Ok(u)
        } else {
          Ok(
            LuaBody::from_value(lua, UserData(x))?
              .map_err(|error| rt_error_fmt!("failed to read body ({error})"))?
              .into_default_response(),
          )
        }
      }
      _ => Err(rt_error_fmt!(
        "cannot convert {} to response",
        value.type_name()
      )),
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

pub fn create_fn_http_create_response(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:http.Response", |lua, mut args: MultiValue| {
    let params: Table =
      check_value(lua, args.pop_front(), "table").map_err(tag_handler(lua, 1, 0))?;
    let body = LuaBody::from_value(lua, params.raw_get::<_, mlua::Value>("body")?)?
      .map_err(|error| bad_field("body", error))?;
    let mut response = body.into_default_response();

    // TODO: better error message for status code
    let status: Option<u16> = params.check_raw_get(lua, "status", "16-bit integer")?;
    if let Some(x) = status {
      response.status =
        StatusCode::from_u16(x).map_err(|_| rt_error_fmt!("invalid status code: {x}"))?;
    }

    let headers_table: Option<Table> = params.check_raw_get(lua, "headers", "table")?;
    if let Some(t) = headers_table {
      response.headers.borrow_mut().extend(check_headers(lua, t)?)
    }

    Ok(response)
  })
}
