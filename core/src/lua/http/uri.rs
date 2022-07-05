use crate::lua::error::{check_string, check_userdata, rt_error_fmt, tag_handler};
use crate::lua::LuaCacheExt;
use mlua::Value::Nil;
use mlua::{ExternalResult, FromLua, Function, Lua, MultiValue, UserData};
use std::collections::HashMap;

#[derive(Debug)]
pub struct LuaUri(pub(crate) hyper::Uri);

impl UserData for LuaUri {
  fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_field_method_get("scheme", |lua, this| lua.pack(this.0.scheme_str()));
    fields.add_field_method_get("host", |lua, this| lua.pack(this.0.host()));
    fields.add_field_method_get("port", |_lua, this| Ok(this.0.port_u16()));
    fields.add_field_method_get("path", |lua, this| lua.pack(this.0.path()));
    fields.add_field_method_get("query_string", |lua, this| lua.pack(this.0.query()));
  }

  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_method("__tostring", |_lua, this, ()| Ok(this.0.to_string()));

    // TODO: support more complex QS structure (e.g. multiple queries with the same
    // name)
    methods.add_function("query", |lua, mut args: MultiValue| {
      let this = check_userdata::<Self>(args.pop_front(), "URI").map_err(tag_handler(lua, 1))?;
      let result = (this.borrow_borrowed().0.query())
        .map(serde_qs::from_str::<HashMap<String, String>>)
        .transpose()
        .map(Option::unwrap_or_default);
      match result {
        Ok(query_map) => lua.pack_multi(query_map),
        Err(error) => lua.pack_multi((Nil, error.to_string())),
      }
    });
  }
}

impl<'lua> FromLua<'lua> for LuaUri {
  fn from_lua(value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    match value {
      mlua::Value::String(s) => Ok(Self(
        hyper::Uri::try_from(s.as_bytes())
          .map_err(|error| rt_error_fmt!("failed to parse URI ({error})"))?,
      )),
      mlua::Value::UserData(x) => {
        let x = x.borrow::<Self>()?;
        Ok(Self(x.0.clone()))
      }
      _ => Err(rt_error_fmt!(
        "failed to parse URI (string expected, got {})",
        value.type_name(),
      )),
    }
  }
}

pub fn create_fn_http_create_uri(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:http.Uri", |lua, mut args: MultiValue| {
    let s = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1))?;
    Ok(LuaUri(hyper::Uri::try_from(s.as_bytes()).to_lua_err()?))
  })
}
