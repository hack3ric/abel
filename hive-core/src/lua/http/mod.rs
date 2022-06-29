mod body;
mod header_map;
mod request;
mod response;
mod uri;

pub use request::LuaRequest;
pub use response::LuaResponse;

use self::request::check_request;
use super::error::{extract_error_async, rt_error_fmt, ExternalResultExt};
use bstr::ByteSlice;
use hyper::client::HttpConnector;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Client, HeaderMap};
use hyper_tls::HttpsConnector;
use mlua::{Function, Lua, MultiValue, Table};
use once_cell::sync::Lazy;
use response::create_fn_create_response;
use uri::create_fn_create_uri;

static CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

pub fn create_preload_http(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(move |lua, ()| {
    let http = lua.create_table()?;

    http.raw_set("request", create_fn_request(lua)?)?;
    http.raw_set("Response", create_fn_create_response(lua)?)?;
    http.raw_set("Uri", create_fn_create_uri(lua)?)?;

    Ok(http)
  })
}

fn create_fn_request(lua: &Lua) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, args: MultiValue| async move {
    let req = check_request(lua, &args, 1, 1)?;
    extract_error_async(lua, async move {
      let resp = CLIENT.request(req.into()).await.to_rt_error()?;
      let resp = LuaResponse::from_hyper(resp);
      Ok(resp)
    })
    .await
  })
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
