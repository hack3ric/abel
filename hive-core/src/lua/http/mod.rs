mod body;
mod request;
mod response;
mod uri;

pub use request::LuaRequest;
pub use response::LuaResponse;

use self::uri::create_fn_create_uri;
use crate::permission::{Permission, PermissionSet};
use hyper::client::HttpConnector;
use hyper::Client;
use hyper_tls::HttpsConnector;
use mlua::{ExternalError, ExternalResult, Function, Lua};
use nonzero_ext::nonzero;
use once_cell::sync::Lazy;
use response::create_fn_create_response;
use std::num::NonZeroU16;
use std::sync::Arc;

static CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

pub fn create_preload_http(lua: &Lua, permissions: Arc<PermissionSet>) -> mlua::Result<Function> {
  lua.create_function(move |lua, ()| {
    let http = lua.create_table()?;

    http.raw_set("request", create_fn_request(lua, permissions.clone())?)?;
    http.raw_set("Response", create_fn_create_response(lua)?)?;
    http.raw_set("Uri", create_fn_create_uri(lua)?)?;

    Ok(http)
  })
}

fn create_fn_request(lua: &Lua, permissions: Arc<PermissionSet>) -> mlua::Result<Function> {
  lua.create_async_function(move |_lua, req: LuaRequest| {
    let permissions = permissions.clone();
    async move {
      if let Some(auth) = req.uri.authority() {
        let host = auth.host();
        let port = (auth.port())
          .map(|x| NonZeroU16::new(x.as_u16()).ok_or("port is zero"))
          .unwrap_or_else(|| {
            (req.uri.scheme())
              .map(|x| match x.as_str() {
                "https" => nonzero!(443u16),
                _ => nonzero!(80u16),
              })
              .ok_or("no URI scheme specified")
          })
          .to_lua_err()?;
        permissions.check(&Permission::Net(host.into(), port))?;
      } else {
        return Err("absolute-form URI required".to_lua_err());
      }

      let resp = CLIENT.request(req.into()).await.to_lua_err()?;
      let resp = LuaResponse::from_hyper(resp);
      Ok(resp)
    }
  })
}
