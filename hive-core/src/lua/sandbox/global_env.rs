use crate::lua::response::create_fn_create_response;
use crate::lua::table::create_fn_table_dump;
use crate::Result;
use mlua::{Function, Lua, Table, ToLua};

pub(super) fn modify_global_env(lua: &Lua) -> Result<()> {
  let globals = lua.globals();
  globals.raw_set("create_response", create_fn_create_response(lua)?)?;
  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;
  globals
    .raw_get::<_, Table>("table")?
    .raw_set("dump", create_fn_table_dump(lua)?)?;
  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> Result<Function> {
  Ok(lua.create_function(|lua, ()| std::thread::current().name().to_lua(lua))?)
}
