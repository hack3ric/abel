use super::body::Body;
use hyper::header::HeaderName;
use hyper::http::{HeaderMap, HeaderValue, StatusCode};
use mlua::{
  ExternalError, ExternalResult, FromLua, Function, Lua, Table, UserData, UserDataFields,
};

#[derive(Default)]
pub struct Response {
  pub status: StatusCode,
  pub headers: HeaderMap,
  pub body: Option<Body>,
}

impl Response {
  pub(crate) fn from_hyper(resp: hyper::Response<hyper::Body>) -> Self {
    let (parts, body) = resp.into_parts();
    Self {
      status: parts.status,
      headers: parts.headers,
      body: Some(body.into()),
    }
  }
}

impl UserData for Response {
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
    // TODO: headers
  }
}

impl<'lua> FromLua<'lua> for Response {
  fn from_lua(value: mlua::Value, lua: &Lua) -> mlua::Result<Self> {
    use mlua::Value::*;
    match value {
      x @ Table(_) | x @ Nil | x @ String(_) => Ok(lua.unpack::<Body>(x)?.into_default_response()),
      UserData(x) => {
        if let Ok(mut u) = x.take::<Self>() {
          if u.body.is_none() {
            let t = x.get_named_user_value::<_, Body>("body")?;
            u.body = Some(t);
          }
          Ok(u)
        } else {
          Ok(lua.unpack::<Body>(UserData(x))?.into_default_response())
        }
      }
      _ => Err("cannot convert to response".to_lua_err()),
    }
  }
}

impl From<Response> for hyper::Response<hyper::Body> {
  fn from(x: Response) -> Self {
    let mut builder = hyper::Response::builder().status(x.status);
    *builder.headers_mut().unwrap() = x.headers;
    builder.body(x.body.unwrap().into()).unwrap()
  }
}

pub fn create_fn_create_response(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|_lua, params: Table| {
    let body = params.raw_get::<_, Body>("body")?;
    let mut response = body.into_default_response();

    let status = params.raw_get::<_, Option<u16>>("status")?;
    if let Some(x) = status {
      response.status = StatusCode::from_u16(x)
        .map_err(|_| format!("invalid status code: {x}"))
        .to_lua_err()?;
    }

    let headers = params.raw_get::<_, Option<Table>>("headers")?;
    if let Some(x) = headers {
      for f in x.pairs::<String, String>() {
        let (k, v) = f?;
        response.headers.insert(
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
