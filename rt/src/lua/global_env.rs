use super::error::{create_fn_assert, create_fn_error, create_fn_pcall};
use mlua::{Function, Lua};
use bstr::ByteSlice;

pub(super) fn modify_global_env(lua: &Lua) -> mlua::Result<()> {
  let globals = lua.globals();

  lua.set_named_registry_value("lua_error", globals.raw_get::<_, Function>("error")?)?;
  lua.set_named_registry_value("lua_pcall", globals.raw_get::<_, Function>("pcall")?)?;

  let bstr_debug_fmt = lua.create_function(|_lua, s: mlua::String| {
    Ok(format!("{:?}", s.as_bytes().as_bstr()))
  })?;

  lua
    .load(include_str!("bootstrap.lua"))
    .set_name("@<bootstrap>")?
    .call(bstr_debug_fmt)?;

  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;
  globals.raw_set("error", create_fn_error(lua)?)?;
  globals.raw_set("assert", create_fn_assert(lua)?)?;
  globals.raw_set("pcall", create_fn_pcall(lua)?)?;

  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| lua.pack(std::thread::current().name()))
}
