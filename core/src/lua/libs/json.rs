use crate::lua::error::{
  arg_error, check_string, check_truthiness, check_value, rt_error, tag_handler,
};
use crate::lua::LuaCacheExt;
use mlua::{Function, Lua, LuaSerdeExt, MultiValue, Table};

pub fn create_preload_json(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:preload_json", |lua, ()| {
    let json_table = lua.create_table()?;
    json_table.raw_set("parse", create_fn_json_parse(lua)?)?;
    json_table.raw_set("stringify", create_fn_json_stringify(lua)?)?;
    json_table.raw_set("array", create_fn_json_array(lua)?)?;
    json_table.raw_set("undo_array", create_fn_json_undo_array(lua)?)?;
    json_table.raw_set("array_metatable", lua.array_metatable())?;
    Ok(json_table)
  })
}

pub(crate) fn create_fn_json_parse(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.parse", |lua, mut args: MultiValue| {
    let string = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 0))?;
    serde_json::from_slice::<serde_json::Value>(string.as_bytes())
      .map_err(rt_error)
      .and_then(|x| lua.to_value(&x))
  })
}

fn create_fn_json_stringify(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.stringify", |lua, mut args: MultiValue| {
    let value = args
      .pop_front()
      .ok_or_else(|| arg_error(lua, 1, "value expected", 0))?;
    let pretty = check_truthiness(args.pop_front());
    let result = if pretty {
      serde_json::to_string_pretty(&value)
    } else {
      serde_json::to_string(&value)
    };
    result.map_err(rt_error)
  })
}

fn create_fn_json_array(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.array", |lua, mut args: MultiValue| {
    let table: Table =
      check_value(lua, args.pop_front(), "table").map_err(tag_handler(lua, 1, 0))?;
    table.set_metatable(Some(lua.array_metatable()));
    Ok(table)
  })
}

fn create_fn_json_undo_array(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.undo_array", |lua, mut args: MultiValue| {
    let table: Table =
      check_value(lua, args.pop_front(), "table").map_err(tag_handler(lua, 1, 0))?;
    if table
      .get_metatable()
      .map(|x| x == lua.array_metatable())
      .unwrap_or(false)
    {
      table.set_metatable(None);
    }
    Ok(table)
  })
}
