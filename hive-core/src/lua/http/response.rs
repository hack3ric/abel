use crate::lua::byte_stream::ByteStream;
use hyper::header::HeaderName;
use hyper::http::{HeaderMap, HeaderValue, StatusCode};
use hyper::Body;
use mlua::{
  AnyUserData, ExternalError, ExternalResult, Function, Lua, Table, ToLua, UserData, UserDataFields,
};

pub struct Response {
  pub status: StatusCode,
  pub headers: HeaderMap,
  pub body: Option<Body>,
}

impl Response {
  pub(crate) fn from_value(value: mlua::Value) -> mlua::Result<Self> {
    use mlua::Value::*;
    match value {
      Table(_) => {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        Ok(Self {
          status: StatusCode::OK,
          headers,
          body: Some(serde_json::to_value(value).to_lua_err()?.to_string().into()),
        })
      }
      UserData(x) => {
        // move user value out
        let mut u = x.take::<Self>()?;
        if u.body.is_none() {
          let t = x.get_named_user_value::<_, AnyUserData>("body")?;
          let t = t.take::<ByteStream>()?;
          u.body = Some(Body::wrap_stream(t.0));
        }
        Ok(u)
      }
      _ => Err("cannot convert to response".to_lua_err()),
    }
  }

  pub(crate) fn from_hyper(resp: hyper::Response<Body>) -> Self {
    let (parts, body) = resp.into_parts();
    Self {
      status: parts.status,
      headers: parts.headers,
      body: Some(body),
    }
  }
}

impl UserData for Response {
  fn add_fields<'lua, F: UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_field_method_get("status", |_lua, this| Ok(this.status.as_u16()));
    fields.add_field_function_get("body", |lua, this| {
      let mut this_ = this.borrow_mut::<Self>()?;
      let body = this_.body.take();
      if let Some(body) = body {
        let x = ByteStream::from_body(body).to_lua(lua)?;
        this.set_named_user_value("body", x.clone())?;
        Ok(x)
      } else {
        this.get_named_user_value("body")
      }
    });
    // TODO: headers
  }
}

impl From<Response> for hyper::Response<Body> {
  fn from(x: Response) -> Self {
    let mut builder = hyper::Response::builder().status(x.status);
    *builder.headers_mut().unwrap() = x.headers;
    builder.body(x.body.unwrap()).unwrap()
  }
}

pub fn create_fn_create_response(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|_lua, params: Table| {
    let status = params
      .raw_get::<_, Option<u16>>("status")?
      .map(|f| {
        StatusCode::from_u16(f)
          .map_err(|_| format!("invalid status code: {}", f))
          .to_lua_err()
      })
      .unwrap_or(Ok(StatusCode::OK))?;

    let mut headers = params
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

    // TODO: byte stream as body
    let body = if let Some(body) = params.raw_get::<_, Option<mlua::Value>>("body")? {
      serde_json::to_value(body).to_lua_err()?.to_string().into()
    } else {
      Body::empty()
    };
    let body = Some(body);

    headers.insert("Content-Type", HeaderValue::from_static("application/json"));

    Ok(Response {
      status,
      headers,
      body,
    })
  })
}
