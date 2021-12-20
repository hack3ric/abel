use std::collections::HashMap;
mod global_env;
mod local_env;

use super::LuaTableExt;
use crate::path::PathMatcher;
use crate::service::Service;
use crate::source::Source;
use crate::Error::*;
use crate::HiveResult;
use global_env::modify_global_env;
use local_env::create_local_env;
use mlua::{Lua, RegistryKey, Table};
use std::backtrace::Backtrace;

#[derive(Debug)]
pub struct Sandbox {
  lua: Lua,
  loaded: HashMap<Box<str>, LoadedService>,
}

#[derive(Debug)]
struct LoadedService {
  service: Service,
  local_env: RegistryKey,
  internal: RegistryKey,
}

impl Sandbox {
  pub fn new() -> HiveResult<Self> {
    let lua = Lua::new();
    let loaded = HashMap::new();
    modify_global_env(&lua)?;
    Ok(Self { lua, loaded })
  }
}

// Creating and loading services
impl Sandbox {
  async fn run_source<'a>(
    &'a self,
    name: &str,
    source: Source,
  ) -> HiveResult<(RegistryKey, RegistryKey, Table<'a>)> {
    let (local_env, internal) = create_local_env(&self.lua, name)?;
    let main = source.get("/main.lua").unwrap();
    self
      .lua
      .load(main)
      .set_environment(local_env.clone())?
      .set_name("<service>/main.lua")?
      .exec_async()
      .await?;
    internal.raw_set("sealed", true)?;
    let local_env_key = self.lua.create_registry_value(local_env)?;
    let internal_key = self.lua.create_registry_value(internal.clone())?;
    Ok((local_env_key, internal_key, internal))
  }

  /// Extracts information from the code, but does not create the service yet
  pub(crate) async fn pre_create_service(
    &self,
    name: &str,
    source: Source,
  ) -> HiveResult<(Vec<PathMatcher>, RegistryKey, RegistryKey)> {
    // TODO: name check
    let (local_env, internal_key, internal) = self.run_source(name, source).await?;

    let mut paths = Vec::new();
    for f in internal
      .raw_get_path::<Table>("<internal>", &["paths"])?
      .sequence_values::<Table>()
    {
      let path = f?.raw_get::<_, String>(1u8)?;
      let path = PathMatcher::new(&path)?;
      paths.push(path);
    }

    Ok((paths, local_env, internal_key))
  }

  pub(crate) async fn finish_create_service(
    &mut self,
    name: &str,
    service: Service,
    local_env: RegistryKey,
    internal: RegistryKey,
  ) -> HiveResult<()> {
    if service.is_dropped() {
      return Err(ServiceDropped {
        backtrace: Backtrace::capture(),
      });
    }
    let loaded = LoadedService {
      service,
      local_env,
      internal,
    };
    self.loaded.insert(name.into(), loaded);
    Ok(())
  }

  async fn load_service(&mut self, service: Service) -> HiveResult<&LoadedService> {
    let service_guard = service.try_upgrade()?;
    let name = service_guard.name();
    if let Some((name_owned, loaded)) = self.loaded.remove_entry(name) {
      if !loaded.service.is_dropped() && loaded.service.ptr_eq(&service) {
        self.loaded.insert(name_owned, loaded);
        return Ok(self.loaded.get(name).unwrap());
      }
    }
    let source = service_guard.source();
    let (local_env, internal, _) = self.run_source(name, source.clone()).await?;
    let loaded = LoadedService {
      service: service.clone(),
      local_env,
      internal,
    };
    self.loaded.insert(name.into(), loaded);
    Ok(&self.loaded[name])
  }
}

// Handling requests
impl Sandbox {
  pub async fn handle_request(&mut self, service: Service) -> HiveResult<()> {
    let loaded = self.load_service(service).await?;
    Ok(())
  }
}
