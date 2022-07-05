use super::error::{arg_error, check_string, check_truthiness, check_value, tag_handler};
use super::LuaCacheExt;
use mlua::Value::Nil;
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

fn create_fn_json_parse(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.parse", |lua, mut args: MultiValue| {
    let string = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1))?;
    let result = serde_json::from_slice::<serde_json::Value>(string.as_bytes());
    match result {
      Ok(result) => lua.pack_multi(lua.to_value(&result)?),
      Err(error) => lua.pack_multi((Nil, error.to_string())),
    }
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
    match result {
      Ok(s) => lua.pack_multi(s),
      Err(error) => lua.pack_multi((Nil, error.to_string())),
    }
  })
}

fn create_fn_json_array(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.array", |lua, mut args: MultiValue| {
    let table: Table = check_value(lua, args.pop_front(), "table").map_err(tag_handler(lua, 1))?;
    table.set_metatable(Some(lua.array_metatable()));
    Ok(table)
  })
}

fn create_fn_json_undo_array(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:json.undo_array", |lua, mut args: MultiValue| {
    let table: Table = check_value(lua, args.pop_front(), "table").map_err(tag_handler(lua, 1))?;
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
