mod body;
mod header_map;
mod request;
mod response;
mod uri;

pub use request::LuaRequest;
pub use response::LuaResponse;
pub(crate) use uri::LuaUri;

use crate::lua::error::{arg_error, check_value, rt_error, rt_error_fmt, tag_error, tag_handler};
use crate::lua::{LuaCacheExt, LuaEither, LUA_HTTP_CLIENT};
use bstr::ByteSlice;
use hyper::header::{HeaderName, HeaderValue};
use hyper::HeaderMap;
use mlua::{AnyUserData, Function, Lua, MultiValue, Table};
use response::create_fn_http_create_response;
use uri::create_fn_http_create_uri;

pub fn create_preload_http(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:preload_http", move |lua, ()| {
    let http = lua.create_table()?;
    http.raw_set("request", create_fn_http_request(lua)?)?;
    http.raw_set("Response", create_fn_http_create_response(lua)?)?;
    http.raw_set("Uri", create_fn_http_create_uri(lua)?)?;
    Ok(http)
  })
}

pub fn create_fn_http_request(lua: &Lua) -> mlua::Result<Function> {
  fn check_request_first_arg(lua: &Lua, value: Option<mlua::Value>) -> mlua::Result<LuaRequest> {
    use LuaEither::*;
    type RequestMeta<'a> = LuaEither<LuaEither<mlua::String<'a>, Table<'a>>, AnyUserData<'a>>;
    const EXPECTED: &str = "URI or request";

    let either =
      check_value::<RequestMeta>(lua, value, EXPECTED).map_err(tag_handler(lua, 1, 1))?;
    match either {
      Left(Left(uri)) => Ok(LuaRequest {
        uri: hyper::Uri::try_from(uri.as_bytes())
          .map_err(|error| arg_error(lua, 1, &error.to_string(), 1))?,
        ..Default::default()
      }),
      Left(Right(table)) => LuaRequest::from_table(lua, table),
      Right(u) if u.is::<LuaRequest>() => LuaRequest::from_userdata(lua, u),
      Right(u) if u.is::<LuaUri>() => Ok(LuaRequest {
        uri: u.borrow::<LuaUri>()?.0.clone(),
        ..Default::default()
      }),
      Right(_) => Err(tag_error(lua, 1, EXPECTED, "other userdata", 1)),
    }
  }

  lua.create_cached_async_function(
    "abel:http.request",
    move |lua, mut args: MultiValue| async move {
      let req = check_request_first_arg(lua, args.pop_front())?;
      LUA_HTTP_CLIENT
        .request(req.into())
        .await
        .map(LuaResponse::from_hyper)
        .map_err(rt_error)
    },
  )
}

fn check_headers(lua: &Lua, headers_table: Table) -> mlua::Result<HeaderMap> {
  let mut headers = HeaderMap::new();
  for entry in headers_table.pairs::<mlua::Value, mlua::Value>() {
    let (key, value) = entry?;
    let type_name = key.type_name();
    let key: mlua::String = lua
      .unpack(key)
      .map_err(|_| rt_error_fmt!("expected string as header name, found {type_name}"))?;
    let key = header_name_convenient(key)?;
    match value {
      mlua::Value::Table(values) => {
        for value in values.sequence_values::<mlua::Value>() {
          let value = value?;
          let type_name = value.type_name();
          let string = lua
            .coerce_string(value)?
            .ok_or_else(|| rt_error_fmt!("expected string as header value, got {type_name}"))?;
          headers.append(&key, header_value(string)?);
        }
      }
      _ => {
        let type_name = value.type_name();
        if let Some(string) = lua.coerce_string(value)? {
          headers.append(key, header_value(string)?);
        } else {
          return Err(rt_error_fmt!(
            "expected string or an array of strings \
            as header value(s), got {type_name}",
          ));
        }
      }
    }
  }
  Ok(headers)
}

fn header_name(name: mlua::String) -> mlua::Result<HeaderName> {
  let name = name.as_bytes();
  HeaderName::from_bytes(name)
    .map_err(|_| rt_error_fmt!("invalid header name: {:?}", name.as_bstr()))
}

fn header_name_convenient(name: mlua::String) -> mlua::Result<HeaderName> {
  let bytes = name.as_bytes();
  if bytes.starts_with(b"@") {
    header_name(name)
  } else {
    let name = bytes.replace("_", "-");
    HeaderName::from_bytes(&name)
      .map_err(|_| rt_error_fmt!("invalid header name: {:?}", name.as_bstr()))
  }
}

fn header_value(value: mlua::String) -> mlua::Result<HeaderValue> {
  let value = value.as_bytes();
  HeaderValue::from_bytes(value)
    .map_err(|_| rt_error_fmt!("invalid header value: {:?}", value.as_bstr()))
}
