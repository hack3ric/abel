use super::body::LuaBody;
use super::uri::LuaUri;
use crate::path::Params;
use hyper::header::{HeaderName, HeaderValue};
use hyper::http::request::Parts;
use hyper::{Body, HeaderMap, Method, Request};
use mlua::{ExternalError, ExternalResult, FromLua, Lua, Table, UserData};

pub struct LuaRequest {
  pub(crate) method: Method,
  /// Must be absolute
  pub(crate) uri: hyper::Uri,
  pub(crate) headers: HeaderMap,
  pub(crate) body: Option<LuaBody>,
  /// Only used in Hive core
  params: Option<Params>,
}

impl LuaRequest {
  #[rustfmt::skip]
  pub fn new(req: Request<Body>, params: Params) -> Self {
    let (Parts { method, uri, headers, .. }, body) = req.into_parts();
    let params = Some(params);
    Self { method, uri, headers, body: Some(body.into()), params }
  }
}

impl Default for LuaRequest {
  fn default() -> Self {
    Self {
      method: Method::GET,
      uri: hyper::Uri::default(),
      headers: HeaderMap::new(),
      body: Some(LuaBody::Empty),
      params: None,
    }
  }
}

impl UserData for LuaRequest {
  fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_field_function_get("params", |lua, this| {
      this
        .get_named_user_value::<_, Table>("params")
        .or_else(|_err| {
          let mut this_ref = this.borrow_mut::<Self>()?;
          let params = this_ref
            .params
            .take()
            .map(|x| {
              let iter = x
                .into_iter()
                .map(|(k, v)| (k.into_string(), v.into_string()));
              lua.create_table_from(iter)
            })
            .unwrap_or_else(|| lua.create_table())?;
          this.set_named_user_value("params", params.clone())?;
          Ok(params)
        })
    });

    fields.add_field_method_get("method", |lua, this| lua.pack(this.method.as_str()));
    fields.add_field_method_get("uri", |_lua, this| Ok(LuaUri(this.uri.clone())));

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

impl<'lua> FromLua<'lua> for LuaRequest {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    match lua_value {
      mlua::Value::String(uri) => Ok(Self {
        uri: hyper::Uri::try_from(uri.as_bytes()).to_lua_err()?,
        ..Default::default()
      }),
      mlua::Value::Table(table) => {
        let method = table
          .raw_get::<_, Option<mlua::String>>("method")?
          .map(|x| Method::from_bytes(x.as_bytes()))
          .transpose()
          .to_lua_err()?
          .unwrap_or(Method::GET);

        let uri: hyper::Uri = table
          .raw_get::<_, mlua::String>("uri")?
          .as_bytes()
          .try_into()
          .to_lua_err()?;

        let headers_table: Option<Table> = table.raw_get("headers")?;
        let mut headers = HeaderMap::new();
        if let Some(headers_table) = headers_table {
          for entry in headers_table.pairs::<mlua::String, mlua::Value>() {
            let (k, v) = entry?;
            let k = HeaderName::from_bytes(k.as_bytes()).to_lua_err()?;
            match v {
              mlua::Value::String(v) => {
                headers.append(k, HeaderValue::from_bytes(v.as_bytes()).to_lua_err()?);
              }
              mlua::Value::Table(vs) => {
                for v in vs.sequence_values::<mlua::String>() {
                  let v = v?;
                  headers.append(&k, HeaderValue::from_bytes(v.as_bytes()).to_lua_err()?);
                }
              }
              _ => return Err("expected string or table".to_lua_err()),
            }
          }
        }

        // TODO: body

        Ok(Self {
          method,
          uri,
          headers,
          ..Default::default()
        })
      }
      mlua::Value::UserData(x) => {
        let mut u = x.take::<Self>()?;
        if u.body.is_none() {
          let t = x.get_named_user_value::<_, LuaBody>("body")?;
          u.body = Some(t);
        }
        Ok(u)
      }
      _ => Err("expected string or table".to_lua_err()),
    }
  }
}

impl From<LuaRequest> for Request<Body> {
  fn from(x: LuaRequest) -> Self {
    let mut builder = Request::builder().method(x.method).uri(x.uri);
    *builder.headers_mut().unwrap() = x.headers;
    builder.body(x.body.unwrap().into()).unwrap()
  }
}
