use crate::path::Params;
use hyper::http::request::Parts;
use mlua::{Table, ToLua, UserData};

pub struct Request {
  params: Option<Params>,
  parts: Parts,
}

impl Request {
  pub fn new<T>(params: Params, req: hyper::Request<T>) -> Self {
    Self {
      params: Some(params),
      parts: req.into_parts().0,
    }
  }
}

impl UserData for Request {
  fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_field_function_get("params", |lua, this| {
      this
        .get_named_user_value::<_, Table>("params")
        .or_else(|_err| {
          let mut this_ref = this.borrow_mut::<Self>()?;
          let params = this_ref
            .params
            .take()
            .map(|x| {
              let iter = x
                .into_iter()
                .map(|(k, v)| (k.into_string(), v.into_string()));
              lua.create_table_from(iter)
            })
            .unwrap_or_else(|| lua.create_table())?;
          this.set_named_user_value("params", params.clone())?;
          Ok(params)
        })
    });

    fields.add_field_method_get("method", |lua, this| this.parts.method.as_str().to_lua(lua));

    // TODO: headers
  }
}
