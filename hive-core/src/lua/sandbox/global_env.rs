use crate::lua::response::create_fn_create_response;
use crate::lua::table::{
  create_fn_table_dump, create_fn_table_insert_shared_3, create_fn_table_scope,
};
use crate::Result;
use mlua::{Function, Lua, Table, ToLua};

pub(super) fn modify_global_env(lua: &Lua) -> Result<()> {
  lua.set_named_registry_value(
    "local_env_fn",
    lua
      .load(include_str!("local_env.lua"))
      .set_name("<local_env>")?
      .into_function()?,
  )?;

  let globals = lua.globals();
  globals.raw_set("create_response", create_fn_create_response(lua)?)?;
  globals.raw_set("current_worker", create_fn_current_worker(lua)?)?;

  let table_module = globals.raw_get::<_, Table>("table")?;
  table_module.raw_set("dump", create_fn_table_dump(lua)?)?;
  table_module.raw_set("insert", create_fn_table_insert(lua)?)?;
  table_module.raw_set("scope", create_fn_table_scope(lua)?)?;

  Ok(())
}

fn create_fn_current_worker(lua: &Lua) -> Result<Function> {
  Ok(lua.create_function(|lua, ()| std::thread::current().name().to_lua(lua))?)
}

fn create_fn_table_insert(lua: &Lua) -> Result<Function> {
  let table_insert_shared_3 = create_fn_table_insert_shared_3(lua)?;
  let table_insert = mlua::chunk! {
    local old_table_insert = table.insert
    local function insert(t, ...)
      if type(t) == "table" then
        return old_table_insert(t, ...)
      elseif type(t) == "userdata" then
        local len = select("#", ...)
        if len == 1 then
          t[#t + 1] = ...
        elseif len == 2 then
          $table_insert_shared_3(t, ...)
        else
          error "wrong number of arguments"
        end
      else
        error "expected table or shared table"
      end
    end
    return insert
  };
  Ok(lua.load(table_insert).call(())?)
}
