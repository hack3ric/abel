use crate::lua::service::ServiceBridge;
use hyper::client::HttpConnector;
use hyper::{Client, Method, Request, Uri, Body};
use hyper_tls::HttpsConnector;
use mlua::{ExternalResult, String as LuaString, ExternalError};
use once_cell::sync::Lazy;
use crate::permission::Permission;

static CLIENT: Lazy<Client<HttpsConnector<HttpConnector>>> =
  Lazy::new(|| Client::builder().build(HttpsConnector::new()));

pub fn add_methods<'lua, M: mlua::UserDataMethods<'lua, ServiceBridge>>(methods: &mut M) {
  methods.add_async_method("request", |lua, this, obj: mlua::Value| async move {
    let (method, url): (_, LuaString) = match obj {
      mlua::Value::String(url) => (Method::GET, url),
      mlua::Value::Table(table) => {
        let method = table
          .raw_get::<_, Option<LuaString>>("method")?
          .map(|x| Method::from_bytes(x.as_bytes()))
          .transpose()
          .to_lua_err()?
          .unwrap_or(Method::GET);
        let url = table.raw_get("url")?;
        (method, url)
      }
      _ => return Err("expected table or string".to_lua_err()),
    };

    let url = Uri::try_from(url.as_bytes()).to_lua_err()?;
    let host = url.host().ok_or("no host provided".to_lua_err())?;
    if !this.upgrade().check(&Permission::net(host, url.port_u16().unwrap_or(0))) {
      return Err("unauthorized".to_lua_err());
    }

    let req = Request::builder().method(method).uri(url).body(Body::empty()).to_lua_err()?;
    let resp = CLIENT.request(req).await.to_lua_err()?;

    // TODO: finish this

    Ok(())
  });
}
