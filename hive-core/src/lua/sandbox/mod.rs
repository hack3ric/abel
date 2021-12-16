use std::collections::HashMap;
mod global_env;
mod local_env;

use crate::service::Service;
use crate::source::Source;
use crate::HiveResult;
use global_env::modify_global_env;
use mlua::{Function, Lua, RegistryKey, Table};

#[derive(Debug)]
pub struct Sandbox {
  lua: Lua,
  loaded: HashMap<Box<str>, LoadedService>,
}

#[derive(Debug)]
struct LoadedService {
  service: Service,
  paths: Vec<(Box<str>, RegistryKey)>,
}

impl Sandbox {
  pub fn new() -> HiveResult<Self> {
    let lua = Lua::new();
    let loaded = HashMap::new();
    modify_global_env(&lua);
    Ok(Self { lua, loaded })
  }

  /// Extracts information from the code, but does not create the service yet
  pub(crate) async fn pre_create_service(
    &self,
    name: &str,
    source: Source,
  ) -> HiveResult<Vec<(Box<str>, RegistryKey)>> {
    let (local_env, internal) = self.create_local_env(name)?;
    let main = source.get("/main.lua").unwrap();
    self
      .lua
      .load(main)
      .set_environment(local_env)?
      .set_name("<service>/main.lua")?
      .exec_async()
      .await?;
    internal.raw_set("sealed", true)?;
    let mut paths = Vec::new();
    for f in internal
      .raw_get::<_, Table>("paths")?
      .sequence_values::<Table>()
    {
      let f = f?;
      let (path, handler) = (f.raw_get::<_, String>(1)?, f.raw_get::<_, Function>(2)?);
      let handler_key = self.lua.create_registry_value(handler)?;
      paths.push((path.into_boxed_str(), handler_key));
    }
    Ok(paths)
  }

  pub(crate) async fn finish_create_service(
    &mut self,
    name: &str,
    service: Service,
    paths: Vec<(Box<str>, RegistryKey)>,
  ) -> HiveResult<()> {
    let loaded = LoadedService { service, paths };
    self.loaded.insert(name.into(), loaded);
    Ok(())
  }
}
