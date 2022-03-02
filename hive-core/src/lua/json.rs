use mlua::{ExternalResult, Function, Lua, LuaSerdeExt, String as LuaString};

pub fn create_preload_json(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| {
    let json_table = lua.create_table()?;
    json_table.raw_set("parse", create_fn_json_parse(lua)?)?;
    json_table.raw_set("stringify", create_fn_json_stringify(lua)?)?;
    Ok(json_table)
  })
}

fn create_fn_json_parse(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, string: LuaString| {
    let result: serde_json::Value = serde_json::from_slice(string.as_bytes()).to_lua_err()?;
    lua.to_value(&result)
  })
}

fn create_fn_json_stringify(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, value: mlua::Value| {
    let result = lua.from_value::<serde_json::Value>(value)?;
    Ok(result.to_string())
  })
}