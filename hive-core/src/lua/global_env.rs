use super::error::create_fn_error;
use mlua::{Function, Lua, Table, ToLua};

pub(super) fn modify_global_env(lua: &Lua) -> mlua::Result<()> {
  let globals = lua.globals();

  lua.set_named_registry_value("lua_error", globals.raw_get::<_, Function>("error")?)?;
  lua.set_named_registry_value("lua_pcall", globals.raw_get::<_, Function>("pcall")?)?;

  lua
    .load(include_str!("global_env.lua"))
    .set_name("<global_env>")?
    .exec()?;

  let routing: Table = lua
    .load(include_str!("routing.lua"))
    .set_name("<routing>")?
    .call(())?;
  globals.raw_set("routing", routing)?;

  lua.set_named_registry_value(
    "isolate_fn",
    lua
      .load(include_str!("isolate.lua"))
      .set_name("<isolate>")?
      .into_function()?,
  )?;

  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;
  globals.raw_set("error", create_fn_error(lua)?)?;

  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| std::thread::current().name().to_lua(lua))
}
