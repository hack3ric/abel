use log::{info, warn};
use mlua::{Function, Lua, MultiValue, Table};

pub fn side_effect_log(name: &str) -> impl FnOnce(&Lua, Table, Table) -> mlua::Result<()> + '_ {
  |lua, env, _| {
    env.raw_set(
      "print",
      create_fn_log(lua, name, |t, s| info!(target: t, "{s}"))?,
    )?;
    env.raw_set(
      "warn",
      create_fn_log(lua, name, |t, s| warn!(target: t, "{s}"))?,
    )
  }
}

fn create_fn_log<'a>(
  lua: &'a Lua,
  service_name: &str,
  f: impl Fn(&str, &str) + 'static,
) -> mlua::Result<Function<'a>> {
  let tostring: Function = lua.globals().raw_get("tostring")?;
  let target = format!("service '{service_name}'");

  let f = lua.create_function(move |_lua, (tostring, mut args): (Function, MultiValue)| {
    let first: mlua::String = tostring.call(args.pop_front())?;
    let first = String::from_utf8_lossy(first.as_bytes()).into_owned();
    let s = args
      .into_iter()
      .try_fold(first, |mut init, x| -> mlua::Result<_> {
        let string: mlua::String = tostring.call(x)?;
        let string = String::from_utf8_lossy(string.as_bytes());
        init.push('\t');
        init.push_str(&string);
        Ok(init)
      })?;
    f(&target, &s);
    Ok(())
  })?;
  f.bind(tostring)
}
