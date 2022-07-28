use super::error::{check_value, create_fn_assert, create_fn_error, create_fn_pcall, tag_handler};
use super::schedule::{create_fn_await_all, create_fn_sleep, create_fn_spawn};
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
    .set_name("@<bootstrap>")?
    .call(bstr_debug_fmt)?;

  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;
  globals.raw_set("spawn", create_fn_spawn(lua)?)?;
  globals.raw_set("sleep", create_fn_sleep(lua)?)?;
  globals.raw_set("await_all", create_fn_await_all(lua)?)?;
  globals.raw_set("bind", create_fn_bind(lua)?)?;

  globals.raw_set("error", create_fn_error(lua)?)?;
  globals.raw_set("assert", create_fn_assert(lua)?)?;
  globals.raw_set("pcall", create_fn_pcall(lua)?)?;

  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| lua.pack(std::thread::current().name()))
}

fn create_fn_bind(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, mut args: MultiValue| {
    check_value::<Function>(lua, args.pop_front(), "function")
      .map_err(tag_handler(lua, 1, 1))?
      .bind(args)
  })
}
