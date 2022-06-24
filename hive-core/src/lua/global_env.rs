use crate::lua::shared::apply_table_module_patch;
use mlua::{Function, Lua, Table, ToLua};

pub(super) fn modify_global_env(lua: &Lua) -> mlua::Result<()> {
  lua
    .load(include_str!("global_env.lua"))
    .set_name("<global_env>")?
    .exec()?;

  let globals = lua.globals();

  let routing: Table = lua
    .load(include_str!("routing.lua"))
    .set_name("<routing>")?
    .call(())?;
  globals.raw_set("routing", routing)?;

  lua.set_named_registry_value(
    "local_env_fn",
    lua
      .load(include_str!("local_env.lua"))
      .set_name("<local_env>")?
      .into_function()?,
  )?;

  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;

  let table_module: Table = globals.raw_get("table")?;
  apply_table_module_patch(lua, table_module)?;

  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| std::thread::current().name().to_lua(lua))
}
