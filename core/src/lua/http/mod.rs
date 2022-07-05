mod body;
mod header_map;
mod request;
mod response;
mod uri;

pub use request::LuaRequest;
pub use response::LuaResponse;

use super::error::rt_error_fmt;
use super::LuaCacheExt;
use crate::lua::error::{arg_error, check_value, tag_handler_async};
use crate::lua::LuaEither;
use bstr::ByteSlice;
use hyper::client::HttpConnector;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Client, HeaderMap};
use hyper_tls::HttpsConnector;
use mlua::Value::Nil;
use mlua::{AnyUserData, Function, Lua, MultiValue, Table};
use once_cell::sync::Lazy;
use response::create_fn_http_create_response;
use uri::create_fn_http_create_uri;

static CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

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
    const EXPECTED: &str = "string, table or request userdata";

    let either =
      check_value::<RequestMeta>(lua, value, EXPECTED).map_err(tag_handler_async(lua, 1))?;
    match either {
      Left(Left(uri)) => Ok(LuaRequest {
        // TODO: uri
        uri: hyper::Uri::try_from(uri.as_bytes())
          .map_err(|error| arg_error(lua, 1, &error.to_string(), 0))?,
        ..Default::default()
      }),
      Left(Right(table)) => LuaRequest::from_table(lua, table),
      Right(userdata) => LuaRequest::from_userdata(userdata),
    }
  }

  lua.create_cached_async_function(
    "abel:http.request",
    move |lua, mut args: MultiValue| async move {
      let req = check_request_first_arg(lua, args.pop_front())?;
      let resp = CLIENT.request(req.into()).await;
      match resp {
        Ok(resp) => lua.pack_multi(LuaResponse::from_hyper(resp)),
        Err(error) => lua.pack_multi((Nil, error.to_string())),
      }
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
