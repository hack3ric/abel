mod body;
mod header_map;
mod request;
mod response;
mod uri;

pub use request::LuaRequest;
pub use response::LuaResponse;
pub(crate) use uri::create_fn_http_create_uri;

use super::error::rt_error_fmt;
use super::LuaCacheExt;
use crate::lua::error::{arg_error, check_value, rt_error, tag_error, tag_handler};
use crate::lua::{LuaEither, LUA_HTTP_CLIENT};
use bstr::ByteSlice;
use hyper::header::{HeaderName, HeaderValue};
use hyper::HeaderMap;
use mlua::{AnyUserData, Function, Lua, MultiValue, Table};
use response::create_fn_http_create_response;
use uri::LuaUri;

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
      Right(u) if u.is::<LuaRequest>() => LuaRequest::from_userdata(u),
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
    let (k, v) = entry?;
    let type_name = k.type_name();
    let k: mlua::String = lua
      .unpack(k)
      .map_err(|_| rt_error_fmt!("expected string as header name, found {type_name}"))?;
    let k = header_name(k)?;
    match v {
      mlua::Value::String(v) => {
        headers.append(k, header_value(v)?);
      }
      mlua::Value::Table(vs) => {
        for v in vs.sequence_values::<mlua::Value>() {
          let v = v?;
          let type_name = v.type_name();
          let v: mlua::String = lua
            .unpack(v)
            .map_err(|_| rt_error_fmt!("expected string as header value, got {type_name}"))?;
          headers.append(&k, header_value(v)?);
        }
      }
      _ => {
        return Err(rt_error_fmt!(
          "expected string or an array of strings as header value(s), got {}",
          v.type_name()
        ))
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

fn header_value(value: mlua::String) -> mlua::Result<HeaderValue> {
  let value = value.as_bytes();
  HeaderValue::from_bytes(value)
    .map_err(|_| rt_error_fmt!("invalid header value: {:?}", value.as_bstr()))
}
