use mlua::{Lua, Table, ToLua};

pub fn set_current(lua: &Lua, context: Option<Table>) -> mlua::Result<()> {
  lua.set_named_registry_value("_hive_current_context", context)
}

pub fn destroy(lua: &Lua, context: Table) -> mlua::Result<()> {
  let code = mlua::chunk! {
    for _, v in ipairs($context) do
      pcall(function()
        local _ <close> = v
      end)
    end
  };
  lua.load(code).set_name("_hive_destroy_context")?.call(())
}

pub fn register<'lua>(lua: &'lua Lua, value: impl ToLua<'lua>) -> mlua::Result<()> {
  let context: Table = lua.named_registry_value("_hive_current_context")?;
  context.raw_insert(context.raw_len() + 1, value)
}
