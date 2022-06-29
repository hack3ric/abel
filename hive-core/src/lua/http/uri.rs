use crate::lua::error::{check_arg, extract_error};
use mlua::{ExternalError, ExternalResult, FromLua, Function, Lua, MultiValue, UserData};
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
    methods.add_function("query", |lua, args: MultiValue| {
      let this = check_arg::<Self>(lua, &args, 1, "URI", 0)?;
      extract_error(lua, || {
        let query_map = (this.0.query())
          .map(serde_qs::from_str::<HashMap<String, String>>)
          .transpose()
          .to_lua_err()?
          .unwrap_or_default();
        Ok(query_map)
      })
    });
  }
}

impl<'lua> FromLua<'lua> for LuaUri {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    match lua_value {
      mlua::Value::String(s) => Ok(Self(hyper::Uri::try_from(s.as_bytes()).to_lua_err()?)),
      mlua::Value::UserData(x) => {
        let x = x.borrow::<Self>()?;
        Ok(Self(x.0.clone()))
      }
      _ => Err("failed to convert to URI".to_lua_err()),
    }
  }
}

pub fn create_fn_create_uri(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, args: MultiValue| {
    let s: mlua::String = check_arg(lua, &args, 1, "string", 0)?;
    Ok(LuaUri(hyper::Uri::try_from(s.as_bytes()).to_lua_err()?))
  })
}
