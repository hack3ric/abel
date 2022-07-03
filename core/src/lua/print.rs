use log::info;
use mlua::{ExternalResult, Function, Lua, MultiValue};

pub fn create_fn_print<'a>(lua: &'a Lua, service_name: &str) -> mlua::Result<Function<'a>> {
  let tostring: Function = lua.globals().raw_get("tostring")?;
  let target = format!("service '{service_name}'");
  let f = lua.create_function(move |_lua, (tostring, args): (Function, MultiValue)| {
    let s = args
      .into_iter()
      .try_fold(String::new(), |mut init, x| -> mlua::Result<_> {
        let string = tostring.call::<_, mlua::String>(x)?;
        let string = std::str::from_utf8(string.as_bytes()).to_lua_err()?;
        init.push_str(string);
        (0..8 - string.as_bytes().len() % 8).for_each(|_| init.push(' '));
        Ok(init)
      })?;
    info!(target: &target, "{s}");
    Ok(())
  })?;
  f.bind(tostring)
}
