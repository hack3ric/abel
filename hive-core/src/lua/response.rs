use http::header::HeaderName;
use http::{HeaderMap, HeaderValue, StatusCode};
use mlua::{ExternalResult, Function, Lua, LuaSerdeExt, Table, UserData};

pub struct Response {
  pub status: StatusCode,
  pub headers: HeaderMap,
  pub body: serde_json::Value,
}

impl UserData for Response {}

pub fn create_fn_create_response(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, params: Table| {
    let status = params
      .raw_get::<_, Option<u16>>("status")?
      .map(|f| {
        StatusCode::from_u16(f)
          .map_err(|_| format!("invalid status code: {}", f))
          .to_lua_err()
      })
      .unwrap_or(Ok(StatusCode::OK))?;

    let headers = params
      .raw_get::<_, Option<Table>>("headers")?
      .map(|t| -> mlua::Result<_> {
        let mut header_map = HeaderMap::new();
        for f in t.pairs::<String, String>() {
          let (k, v) = f?;
          header_map.insert(
            HeaderName::from_bytes(k.as_bytes())
              .map_err(|_| format!("invalid header value: {}", k))
              .to_lua_err()?,
            HeaderValue::from_str(&v)
              .map_err(|_| format!("invalid header value: {}", v))
              .to_lua_err()?,
          );
        }
        Ok(header_map)
      })
      .unwrap_or_else(|| Ok(HeaderMap::new()))?;

    let body = params
      .raw_get::<_, Option<mlua::Value>>("body")?
      .ok_or("missing body in response")
      .to_lua_err()?;
    let body = lua.from_value::<serde_json::Value>(body)?;

    Ok(Response {
      status,
      headers,
      body,
    })
  })
}
