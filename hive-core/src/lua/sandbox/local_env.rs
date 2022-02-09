use crate::lua::context::create_context;
use crate::{Result, Source};
use mlua::{Lua, Table};

pub(super) fn create_local_env<'a>(
  lua: &'a Lua,
  service_name: &str,
  source: Source,
) -> Result<(Table<'a>, Table<'a>)> {
  let (local_env, internal): (Table, Table) = lua.load(include_str!("local_env.lua")).set_name("<local_env>")?.call(())?;

  local_env
    .raw_get::<_, Table>("hive")?
    .raw_set("context", create_context(service_name.into()))?;

  internal.raw_set("source", source)?;

  Ok((local_env, internal))
}
