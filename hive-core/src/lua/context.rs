use mlua::{Lua, RegistryKey, Table, ToLua};

pub fn create(lua: &Lua) -> mlua::Result<RegistryKey> {
  lua.create_registry_value(lua.create_table()?)
}

pub fn set_current(lua: &Lua, context: Option<&RegistryKey>) -> mlua::Result<()> {
  let context = context
    .map(|x| lua.registry_value::<Table>(x))
    .transpose()?;
  lua.set_named_registry_value("_hive_current_context", context)
}

pub fn destroy(lua: &Lua, context: RegistryKey) -> mlua::Result<()> {
  let context_table: Table = lua.registry_value(&context)?;
  lua.remove_registry_value(context)?;
  let code = mlua::chunk! {
    for _, v in ipairs($context_table) do
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
