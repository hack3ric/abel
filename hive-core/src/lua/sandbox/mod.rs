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
use mlua::{Function, Lua, RegistryKey, Table};
use regex::Regex;
use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::lazy::SyncLazy;

static NAME_CHECK_REGEX: SyncLazy<Regex> =
  SyncLazy::new(|| Regex::new("^[a-z0-9-]{1,64}$").unwrap());

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
  /// Extracts information from the code, but does not create the service yet
  pub(crate) async fn pre_create_service(
    &self,
    name: &str,
    source: Source,
  ) -> HiveResult<(Vec<PathMatcher>, RegistryKey, RegistryKey)> {
    if !NAME_CHECK_REGEX.is_match(name) {
      return Err(InvalidServiceName { name: name.into() });
    }

    let (local_env, internal_key, internal) = run_source(&self.lua, name, source).await?;

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
    self.run_start(service.clone()).await?;
    let loaded = LoadedService {
      service,
      local_env,
      internal,
    };
    self.loaded.insert(name.into(), loaded);
    Ok(())
  }

  async fn run_start(&mut self, service: Service) -> HiveResult<()> {
    let loaded = load_service(&self.lua, &mut self.loaded, service).await?;
    let start_fn: Function = self
      .lua
      .registry_value::<Table>(&loaded.local_env)?
      .raw_get_path("<local_env>", &["hive", "start"])?;
    start_fn.call_async(()).await?;
    Ok(())
  }

  async fn run_stop(&mut self, service: Service) -> HiveResult<()> {
    let loaded = load_service(&self.lua, &mut self.loaded, service).await?;
    let stop_fn: Function = self
      .lua
      .registry_value::<Table>(&loaded.local_env)?
      .raw_get_path("<local_env>", &["hive", "stop"])?;
    stop_fn.call_async(()).await?;
    // TODO: Call modules' `stop`
    Ok(())
  }
}

// These methods are separated from `impl` because different mutability of
// references of `lua` and `loaded` is needed.

async fn run_source<'a>(
  self_lua: &'a Lua,
  name: &str,
  source: Source,
) -> HiveResult<(RegistryKey, RegistryKey, Table<'a>)> {
  let (local_env, internal) = create_local_env(self_lua, name)?;
  let main = source.get("/main.lua").unwrap();
  self_lua
    .load(main)
    .set_environment(local_env.clone())?
    .set_name("<service>/main.lua")?
    .exec_async()
    .await?;
  internal.raw_set("sealed", true)?;
  let local_env_key = self_lua.create_registry_value(local_env)?;
  let internal_key = self_lua.create_registry_value(internal.clone())?;
  Ok((local_env_key, internal_key, internal))
}

async fn load_service<'a>(
  self_lua: &'a Lua,
  self_loaded: &'a mut HashMap<Box<str>, LoadedService>,
  service: Service,
) -> HiveResult<&'a LoadedService> {
  let service_guard = service.try_upgrade()?;
  let name = service_guard.name();
  if let Some((name_owned, loaded)) = self_loaded.remove_entry(name) {
    if !loaded.service.is_dropped() && loaded.service.ptr_eq(&service) {
      self_loaded.insert(name_owned, loaded);
      return Ok(self_loaded.get(name).unwrap());
    }
  }
  let source = service_guard.source();
  let (local_env, internal, _) = run_source(self_lua, name, source.clone()).await?;
  let loaded = LoadedService {
    service: service.clone(),
    local_env,
    internal,
  };
  self_loaded.insert(name.into(), loaded);
  Ok(&self_loaded[name])
}
