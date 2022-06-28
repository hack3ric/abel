mod body;
mod header_map;
mod request;
mod response;
mod uri;

pub use request::LuaRequest;
pub use response::LuaResponse;

use super::error::extract_error_async;
use hyper::client::HttpConnector;
use hyper::Client;
use hyper_tls::HttpsConnector;
use mlua::{ExternalResult, Function, Lua};
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
  lua.create_async_function(move |lua, req: LuaRequest| {
    extract_error_async(lua, async move {
      let resp = CLIENT.request(req.into()).await.to_lua_err()?;
      let resp = LuaResponse::from_hyper(resp);
      Ok(resp)
    })
  })
}
