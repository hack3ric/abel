use crate::permission::{Permission, PermissionSet};
use crate::{Request, Response};
use hyper::client::HttpConnector;
use hyper::Client;
use hyper_tls::HttpsConnector;
use mlua::{ExternalError, ExternalResult, Lua, Function};
use once_cell::sync::Lazy;
use std::num::NonZeroU16;
use std::sync::Arc;

static CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

pub fn create_fn_request(lua: &Lua, permissions: Arc<PermissionSet>) -> mlua::Result<Function> {
  lua.create_async_function(move |_lua, req: Request| {
    let permissions = permissions.clone();
    async move {
      if let Some(host) = req.uri.host() {
        let port = (req.uri.port())
          .and_then(|x| NonZeroU16::new(x.as_u16()))
          .or_else(|| {
            req.uri.scheme().and_then(|x| match x.as_str() {
              "http" => Some(unsafe { NonZeroU16::new_unchecked(80) }),
              "https" => Some(unsafe { NonZeroU16::new_unchecked(443) }),
              _ => None,
            })
          });
        permissions.check(&Permission::net(host, port));
      } else {
        return Err(format!("not an absolute URI: {}", req.uri).to_lua_err());
      }

      let resp = CLIENT.request(req.into()).await.to_lua_err()?;
      let resp = Response::from_hyper(resp);
      Ok(resp)
    }
  })
}
