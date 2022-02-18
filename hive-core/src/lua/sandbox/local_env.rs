use crate::lua::context::create_context;
use crate::{Result, Source};
use mlua::{Function, Lua, Table};

pub(super) fn create_local_env<'a>(
  lua: &'a Lua,
  service_name: &str,
  source: Source,
) -> Result<(Table<'a>, Table<'a>)> {
  let local_env_fn = lua.named_registry_value::<_, Function>("local_env_fn")?;
  let (local_env, internal): (Table, Table) = local_env_fn.call(())?;

  local_env
    .raw_get::<_, Table>("hive")?
    .raw_set("context", create_context(service_name.into()))?;
  internal.raw_set("source", source)?;

  Ok((local_env, internal))
}
