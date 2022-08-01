use super::error::{check_value, modify_global_error_handling, tag_handler};
use bstr::ByteSlice;
use mlua::{Function, Lua, MultiValue};

pub(super) fn modify_global_env(lua: &Lua) -> mlua::Result<()> {
  let globals = lua.globals();

  lua.set_named_registry_value("lua_error", globals.raw_get::<_, Function>("error")?)?;
  lua.set_named_registry_value("lua_pcall", globals.raw_get::<_, Function>("pcall")?)?;

  let bstr_debug_fmt =
    lua.create_function(|_lua, s: mlua::String| Ok(format!("{:?}", s.as_bytes().as_bstr())))?;

  lua
    .load(include_str!("bootstrap.lua"))
    .set_name("@[bootstrap]")?
    .call(bstr_debug_fmt)?;

  globals.raw_set("bind", create_fn_bind(lua)?)?;
  modify_global_error_handling(lua)?;

  Ok(())
}

fn create_fn_bind(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, mut args: MultiValue| {
    check_value::<Function>(lua, args.pop_front(), "function")
      .map_err(tag_handler(lua, 1, 1))?
      .bind(args)
  })
}
