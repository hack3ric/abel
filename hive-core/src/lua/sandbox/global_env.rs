use crate::HiveResult;
use mlua::Lua;

pub(super) fn modify_global_env(lua: &Lua) -> HiveResult<()> {
  let globals = lua.globals();
  // There's nothing much I can do here for now
  Ok(())
}
