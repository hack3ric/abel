use crate::Result;
use mlua::{Function, Lua, Table, Value};
use std::collections::HashMap;
use std::lazy::SyncLazy;

pub(super) fn create_local_env(
  lua: &Lua,
  _service_name: impl AsRef<str>,
) -> Result<(Table, Table)> {
  let local_env = lua.create_table()?;
  apply_whitelist(&lua, &local_env)?;

  let internal = lua.create_table()?;
  internal.raw_set("paths", lua.create_table()?)?;
  internal.raw_set("sealed", false)?;

  let hive = lua.create_table()?;
  hive.raw_set("register", create_register_fn(&lua, internal.clone())?)?;
  local_env.raw_set("hive", hive)?;

  Ok((local_env, internal))
}

#[rustfmt::skip]
static LUA_GLOBAL_WHITELIST: SyncLazy<HashMap<&'static str, &'static [&'static str]>> = SyncLazy::new(|| {
  let mut whitelist = HashMap::new();

  whitelist.insert("", &[
    "assert", "error", "ipairs", "next",
    "pairs", "pcall", "print", "rawequal",
    "select", "setmetatable", "tonumber", "tostring",
    "type", "warn", "xpcall", "_VERSION",
  ][..]);

  whitelist.insert("math", &[
    "abs", "acos", "asin", "atan",
    "atan2", "ceil", "cos", "deg",
    "exp", "floor", "fmod", "frexp",
    "huge", "ldexp", "log", "log10",
    "max", "maxinteger", "min", "mininteger",
    "modf", "pi", "pow", "rad", "random",
    "sin", "sinh", "sqrt", "tan",
    "tanh", "tointeger", "type", "ult",
  ][..]);

  whitelist.insert("os", &[
    "clock", "difftime", "time",
  ][..]);

  whitelist.insert("string", &[
    "byte", "char", "find", "format",
    "gmatch", "gsub", "len", "lower",
    "match", "reverse", "sub", "upper",
  ][..]);

  whitelist.insert("table", &[
    "insert", "maxn", "remove", "sort",
  ][..]);

  whitelist
});

fn apply_whitelist(lua: &Lua, local_env: &Table) -> Result<()> {
  let globals = lua.globals();
  for (&k, &v) in LUA_GLOBAL_WHITELIST.iter() {
    if k.is_empty() {
      for &name in v {
        local_env.raw_set(name, globals.raw_get::<_, Value>(name)?)?;
      }
    } else {
      let module: Table = globals.raw_get(k)?;
      let new_module = lua.create_table()?;
      local_env.raw_set(k, new_module.clone())?;
      for &name in v {
        new_module.raw_set(name, module.raw_get::<_, Value>(name)?)?;
      }
    }
  }
  Ok(())
}

fn create_register_fn<'a>(lua: &'a Lua, internal: Table<'a>) -> Result<Function<'a>> {
  let register_fn: Function = lua
    .load(mlua::chunk! {
      return function(path, handler)
        if $internal.sealed then
          error("cannot call `hive.register` from places other than the top level of files")
        end
        table.insert($internal.paths, { path, handler })
      end
    })
    .call(())?;
  Ok(register_fn)
}
