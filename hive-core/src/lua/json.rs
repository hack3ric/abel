use mlua::{ExternalResult, Function, Lua, LuaSerdeExt, Table};

pub fn create_preload_json(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| {
    let json_table = lua.create_table()?;
    json_table.raw_set("parse", create_fn_json_parse(lua)?)?;
    json_table.raw_set("stringify", create_fn_json_stringify(lua)?)?;
    json_table.raw_set("array", create_fn_json_array(lua)?)?;
    json_table.raw_set("array_metatable", lua.array_metatable())?;
    Ok(json_table)
  })
}

fn create_fn_json_parse(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, string: mlua::String| {
    let result: serde_json::Value = serde_json::from_slice(string.as_bytes()).to_lua_err()?;
    lua.to_value(&result)
  })
}

fn create_fn_json_stringify(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, (value, pretty): (mlua::Value, Option<bool>)| {
    let result = lua.from_value::<serde_json::Value>(value)?;
    let string = if pretty.unwrap_or_default() {
      serde_json::to_string_pretty(&result).to_lua_err()?
    } else {
      result.to_string()
    };
    Ok(string)
  })
}

fn create_fn_json_array(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, table: Table| {
    table.set_metatable(Some(lua.array_metatable()));
    Ok(table)
  })
}
