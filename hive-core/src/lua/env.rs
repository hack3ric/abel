use mlua::{ExternalResult, Function, Lua};

pub fn create_fn_os_getenv(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(move |_lua, name: mlua::String| {
    let name = std::str::from_utf8(name.as_bytes()).to_lua_err()?;
    // permissions.check(&Permission::Env {
    //   name: Cow::Borrowed(name),
    // })?;
    // TODO: gate os.getenv
    std::env::var(name).to_lua_err()
  })
}
