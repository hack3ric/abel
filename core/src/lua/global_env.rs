use super::error::{create_fn_assert, create_fn_error, create_fn_pcall};
use mlua::{Function, Lua, Table};

pub(super) fn modify_global_env(lua: &Lua) -> mlua::Result<()> {
  let globals = lua.globals();

  lua.set_named_registry_value("lua_error", globals.raw_get::<_, Function>("error")?)?;
  lua.set_named_registry_value("lua_pcall", globals.raw_get::<_, Function>("pcall")?)?;

  lua
    .load(include_str!("bootstrap.lua"))
    .set_name("<bootstrap>")?
    .exec()?;

  let routing: Table = lua
    .load(include_str!("routing.lua"))
    .set_name("<routing>")?
    .call(())?;
  globals.raw_set("routing", routing)?;

  lua.set_named_registry_value(
    "isolate_fn",
    lua
      .load(include_str!("isolate_bootstrap.lua"))
      .set_name("<isolate_bootstrap>")?
      .into_function()?,
  )?;

  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;
  globals.raw_set("error", create_fn_error(lua)?)?;
  globals.raw_set("assert", create_fn_assert(lua)?)?;
  globals.raw_set("pcall", create_fn_pcall(lua)?)?;

  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| lua.pack(std::thread::current().name()))
}
