use super::body::LuaBody;
use super::header_map::LuaHeaderMap;
use super::uri::LuaUri;
use crate::lua::error::{bad_field, rt_error_fmt, TableCheckExt};
use crate::lua::http::check_headers;
use crate::path::Params;
use crate::task::close_value;
use hyper::http::request::Parts;
use hyper::{Body, HeaderMap, Method, Request, Uri};
use mlua::{AnyUserData, Lua, Table, UserData};
use std::cell::RefCell;
use std::rc::Rc;

pub struct LuaRequest {
  pub(crate) method: Method,
  /// Must be absolute
  pub(crate) uri: Uri,
  pub(crate) headers: Rc<RefCell<HeaderMap>>,
  pub(crate) body: Option<LuaBody>,
  /// Only used in Abel core
  pub(crate) params: Option<Params>,
}

impl LuaRequest {
  #[rustfmt::skip]
  pub fn new(req: Request<Body>, params: Params) -> Self {
    let (Parts { method, uri, headers, .. }, body) = req.into_parts();
    let headers = Rc::new(RefCell::new(headers));
    let body = Some(body.into());
    let params = Some(params);
    Self { method, uri, headers, body, params }
  }

  pub fn from_table<'lua>(lua: &'lua Lua, table: Table<'lua>) -> mlua::Result<LuaRequest> {
    let method = table
      .check_raw_get::<Option<mlua::String>>(lua, "method", "string")?
      .map(|x| {
        let x = x.as_bytes();
        Method::from_bytes(x)
          .map_err(|_| rt_error_fmt!("invalid HTTP method: {}", String::from_utf8_lossy(x)))
      })
      .transpose()?
      .unwrap_or(Method::GET);

    let uri: Uri = table
      .check_raw_get::<mlua::String>(lua, "uri", "string")?
      .as_bytes()
      .try_into()
      .map_err(|error| rt_error_fmt!("invalid URI ({error})"))?;

    let headers_table: Option<Table> = table.check_raw_get(lua, "headers", "table")?;
    let headers = headers_table
      .map(|t| check_headers(lua, t))
      .transpose()?
      .unwrap_or_else(HeaderMap::new);

    let body = LuaBody::from_lua_with_error_msg(lua, table.raw_get::<_, mlua::Value>("body")?)?
      .map_err(|error| bad_field("body", error))?;

    Ok(LuaRequest {
      method,
      uri,
      headers: Rc::new(RefCell::new(headers)),
      body: Some(body),
      ..Default::default()
    })
  }

  pub fn from_userdata(lua: &Lua, userdata: AnyUserData) -> mlua::Result<LuaRequest> {
    let mut u: LuaRequest = userdata.take()?;
    if u.body.is_none() {
      let t = userdata.get_named_user_value::<_, mlua::Value>("body")?;
      let body = LuaBody::from_lua_with_error_msg(lua, t)?
        .map_err(|error| rt_error_fmt!("failed to get body from request ({error})"))?;
      u.body = Some(body);
    }
    Ok(u)
  }
}

impl Default for LuaRequest {
  fn default() -> Self {
    Self {
      method: Method::GET,
      uri: Default::default(),
      headers: Default::default(),
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

    fields.add_field_method_get("headers", |_lua, this| {
      Ok(LuaHeaderMap(this.headers.clone()))
    });
  }

  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_function("__close", |_lua, this: AnyUserData| {
      let _ = this.get_named_user_value("body").and_then(close_value);
      let _ = this.take::<Self>();
      Ok(())
    });
  }
}

impl From<LuaRequest> for Request<Body> {
  fn from(x: LuaRequest) -> Self {
    let headers = Rc::try_unwrap(x.headers)
      .map(RefCell::into_inner)
      .unwrap_or_else(|x| x.borrow().clone());

    let mut builder = Request::builder().method(x.method).uri(x.uri);
    *builder.headers_mut().unwrap() = headers;
    builder.body(x.body.unwrap().into()).unwrap()
  }
}
