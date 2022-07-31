use mlua::Value::Nil;
use mlua::{Function, Lua, MultiValue, Table};

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
  ($($module:ident => $wl:expr;)*) => {
    paste::paste! {
      $(pub fn [<create_preload_ $module>](lua: &Lua) -> mlua::Result<Function> {
        lua.create_function(|lua, ()| {
          let module = lua.create_table()?;
          apply_whitelist(lua.globals().raw_get(stringify!($module))?, module.clone(), $wl)?;
          Ok(module)
        })
      })*
    }
  };
}

pub fn side_effect_global_whitelist(
  lua: &Lua,
  local_env: Table,
  _internal: Table,
) -> mlua::Result<()> {
  let globals = lua.globals();

  apply_whitelist(globals.clone(), local_env.clone(), [
    "assert", "error", "getmetatable", "ipairs", "next", "pairs", "pcall", "print", "rawequal",
    "select", "setmetatable", "tonumber", "tostring", "type", "warn", "xpcall", "_VERSION",
  ])?;

  // Custom functions
  apply_whitelist(globals, local_env, ["debug_fmt", "HttpError", "bind"])
}

create_whitelist_preloads! {
  math => [
    "abs", "acos", "asin", "atan", "atan2", "ceil", "cos", "deg", "exp", "floor", "fmod",
    "frexp", "huge", "ldexp", "log", "log10", "max", "maxinteger", "min", "mininteger", "modf",
    "pi", "pow", "rad", "random", "sin", "sinh", "sqrt", "tan", "tanh", "tointeger", "type",
    "ult",
  ];

  // Removed `string.dump`
  string => [
    "gsub", "format", "byte", "upper", "char", "pack", "lower", "sub", "gmatch", "reverse",
    "match", "len", "rep", "find", "unpack", "packsize",
  ];

  table => [
    "remove", "sort", "move", "concat", "unpack", "insert", "pack",
  ];

  coroutine => [
    "close", "create", "isyieldable", "resume", "running", "status", "wrap", "yield",
  ];

  utf8 => [
    "char", "charpattern", "codes", "codepoint", "len", "offset",
  ];
}

pub fn create_preload_os(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(move |lua, ()| {
    let os = lua.create_table()?;
    apply_whitelist(lua.globals().raw_get("os")?, os.clone(), [
      "clock", "difftime", "time",
    ])?;
    os.raw_set("getenv", create_fn_os_getenv(lua)?)?;
    Ok(os)
  })
}

fn create_fn_os_getenv(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|_lua, _args: MultiValue| {
    // TODO: read env from config file
    Ok(Nil)
  })
}
