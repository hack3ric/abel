use crate::lua::service::ServiceBridge;
use crate::permission::Permission;
use crate::{Request, Response};
use hyper::client::HttpConnector;
use hyper::Client;
use hyper_tls::HttpsConnector;
use mlua::{ExternalError, ExternalResult};
use once_cell::sync::Lazy;
use std::num::NonZeroU16;

static CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

pub fn add_methods<'lua, M: mlua::UserDataMethods<'lua, ServiceBridge>>(methods: &mut M) {
  methods.add_async_method("request", |_lua, this, req: Request| async move {
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
      this.upgrade().check(&Permission::net(host, port));
    } else {
      return Err(format!("not an absolute URI: {}", req.uri).to_lua_err());
    }

    let resp = CLIENT.request(req.into()).await.to_lua_err()?;
    let resp = Response::from_hyper(resp);
    Ok(resp)
  });
}
