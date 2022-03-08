use super::body::Body;
use super::uri::Uri;
use crate::path::Params;
use hyper::header::{HeaderName, HeaderValue};
use hyper::http::request::Parts;
use hyper::{HeaderMap, Method};
use mlua::{ExternalError, ExternalResult, FromLua, Lua, String as LuaString, Table, UserData};

pub struct Request {
  pub(crate) method: Method,
  /// Must be absolute
  pub(crate) uri: hyper::Uri,
  pub(crate) headers: HeaderMap,
  pub(crate) body: Option<Body>,
  /// Only used in Hive core
  params: Option<Params>,
}

impl Request {
  #[rustfmt::skip]
  pub fn new(req: hyper::Request<hyper::Body>, params: Params) -> Self {
    let (Parts { method, uri, headers, .. }, body) = req.into_parts();
    let params = Some(params);
    Self { method, uri, headers, body: Some(body.into()), params }
  }
}

impl Default for Request {
  fn default() -> Self {
    Self {
      method: Method::GET,
      uri: hyper::Uri::default(),
      headers: HeaderMap::new(),
      body: Some(Body::Empty),
      params: None,
    }
  }
}

impl UserData for Request {
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
    fields.add_field_method_get("uri", |_lua, this| Ok(Uri(this.uri.clone())));

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

impl<'lua> FromLua<'lua> for Request {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    match lua_value {
      mlua::Value::String(uri) => Ok(Self {
        uri: hyper::Uri::try_from(uri.as_bytes()).to_lua_err()?,
        ..Default::default()
      }),
      mlua::Value::Table(table) => {
        let method = table
          .raw_get::<_, Option<LuaString>>("method")?
          .map(|x| Method::from_bytes(x.as_bytes()))
          .transpose()
          .to_lua_err()?
          .unwrap_or(Method::GET);

        let uri: hyper::Uri = table
          .raw_get::<_, LuaString>("uri")?
          .as_bytes()
          .try_into()
          .to_lua_err()?;

        let headers_table: Option<Table> = table.raw_get("headers")?;
        let mut headers = HeaderMap::new();
        if let Some(headers_table) = headers_table {
          for entry in headers_table.pairs::<LuaString, mlua::Value>() {
            let (k, v) = entry?;
            let k = HeaderName::from_bytes(k.as_bytes()).to_lua_err()?;
            match v {
              mlua::Value::String(v) => {
                headers.append(k, HeaderValue::from_bytes(v.as_bytes()).to_lua_err()?);
              }
              mlua::Value::Table(vs) => {
                for v in vs.sequence_values::<LuaString>() {
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
          let t = x.get_named_user_value::<_, Body>("body")?;
          u.body = Some(t);
        }
        Ok(u)
      }
      _ => Err("expected string or table".to_lua_err()),
    }
  }
}

impl From<Request> for hyper::Request<hyper::Body> {
  fn from(x: Request) -> Self {
    let mut builder = hyper::Request::builder().method(x.method).uri(x.uri);
    *builder.headers_mut().unwrap() = x.headers;
    builder.body(x.body.unwrap().into()).unwrap()
  }
}
