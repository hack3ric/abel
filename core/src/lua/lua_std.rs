use mlua::{Function, Lua, Table};

fn apply_whitelist<'lua>(
  from: Table<'lua>,
  to: Table<'lua>,
  wl: impl IntoIterator<Item = &'lua str>,
) -> mlua::Result<()> {
  for x in wl {
    to.raw_set(x, from.raw_get::<_, mlua::Value>(x)?)?;
  }
  Ok(())
}

macro_rules! create_whitelist_preloads {
  ($($fn_name:ident => ($module:expr, $wl:expr);)*) => {
    $(pub fn $fn_name(lua: &Lua) -> mlua::Result<Function> {
      lua.create_function(|lua, ()| {
        let module = lua.create_table()?;
        apply_whitelist(lua.globals().raw_get($module)?, module.clone(), $wl)?;
        Ok(module)
      })
    })*
  };
}

pub fn global_whitelist(lua: &Lua, local_env: Table, _internal: Table) -> mlua::Result<()> {
  apply_whitelist(lua.globals(), local_env, [
    "assert", "error", "getmetatable", "ipairs", "next", "pairs", "pcall", "print", "rawequal",
    "select", "setmetatable", "tonumber", "tostring", "type", "warn", "xpcall", "_VERSION",
  ])
}

create_whitelist_preloads! {
  create_preload_math => ("math", [
    "abs", "acos", "asin", "atan", "atan2", "ceil", "cos", "deg", "exp", "floor", "fmod",
    "frexp", "huge", "ldexp", "log", "log10", "max", "maxinteger", "min", "mininteger", "modf",
    "pi", "pow", "rad", "random", "sin", "sinh", "sqrt", "tan", "tanh", "tointeger", "type",
    "ult",
  ]);

  create_preload_string => ("string", [
    "gsub", "format", "byte", "upper", "char", "pack", "lower", "sub", "gmatch", "reverse",
    "match", "len", "rep", "find", "unpack", "packsize",
  ]);

  create_preload_table => ("table", [
    "remove", "sort", "move", "concat", "unpack", "insert", "pack",
  ]);

  create_preload_coroutine => ("coroutine", [
    "close", "create", "isyieldable", "resume", "running", "status", "wrap", "yield",
  ]);
}

pub fn create_preload_os(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| {
    let module = lua.create_table()?;
    apply_whitelist(lua.globals().raw_get("os")?, module.clone(), [
      "clock", "difftime", "time",
    ])?;
    // TODO: add some shim from other modules
    Ok(module)
  })
}
