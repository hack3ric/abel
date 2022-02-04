mod global_env;
mod local_env;
mod package;

use super::context::remove_service_contexts;
use super::LuaTableExt;
use crate::path::PathMatcher;
use crate::service::Service;
use crate::source::Source;
use crate::ErrorKind::*;
use crate::{Request, Response, Result};
use global_env::modify_global_env;
use hyper::Body;
use local_env::create_local_env;
use mlua::{Function, Lua, RegistryKey, Table};
use once_cell::sync::Lazy;
use regex::Regex;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;

static NAME_CHECK_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new("^[a-z0-9-]{1,64}$").unwrap());

#[derive(Debug)]
pub struct Sandbox {
  lua: Lua,
  loaded: RefCell<HashMap<Box<str>, LoadedService>>,
}

#[derive(Debug)]
struct LoadedService {
  service: Service,
  local_env: RegistryKey,
  internal: RegistryKey,
}

impl Sandbox {
  pub fn new() -> Result<Self> {
    let lua = Lua::new();
    let loaded = RefCell::new(HashMap::new());
    modify_global_env(&lua)?;
    Ok(Self { lua, loaded })
  }
}

// Creating and loading services
impl Sandbox {
  pub async fn run(
    &self,
    service: Service,
    path: &str,
    req: hyper::Request<Body>,
  ) -> Result<Response> {
    let guard = service.try_upgrade()?;
    let (params, matcher) = guard
      .paths()
      .iter()
      .find_map(|m| m.gen_params(path).map(|p| (p, m)))
      .ok_or_else(|| PathNotFound {
        service: guard.name().into(),
        path: path.into(),
      })?;

    let loaded = self.load_service(service.clone()).await?;
    let internal: Table = self.lua.registry_value(&loaded.internal)?;

    for f in internal
      .raw_get_path::<Table>("<internal>", &["paths"])?
      .sequence_values::<Table>()
    {
      let f = f?;
      let path = f.raw_get::<u8, String>(1)?;
      if path == matcher.as_str() {
        let handler = f.raw_get::<u8, Function>(2)?;
        let req = Request::new(params, req);
        let result: mlua::Value = handler.call_async(req).await?;
        let resp = Response::from_value(&self.lua, result)?;
        return Ok(resp);
      }
    }
    panic!("path matched but no handler found; this is a bug")
  }

  /// Extracts information from the code, but does not create the service yet
  pub(crate) async fn pre_create_service(
    &self,
    name: &str,
    source: Source,
  ) -> Result<(Vec<PathMatcher>, RegistryKey, RegistryKey)> {
    if !NAME_CHECK_REGEX.is_match(name) {
      return Err(InvalidServiceName(name.into()).into());
    }

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
    &self,
    name: &str,
    service: Service,
    local_env: RegistryKey,
    internal: RegistryKey,
  ) -> Result<()> {
    if service.is_dropped() {
      return Err(ServiceDropped.into());
    }
    self.run_start(service.clone()).await?;
    let loaded = LoadedService {
      service,
      local_env,
      internal,
    };
    self.loaded.borrow_mut().insert(name.into(), loaded);
    Ok(())
  }

  async fn run_start(&self, service: Service) -> Result<()> {
    let loaded = self.load_service(service).await?;
    let start_fn: Option<Function> = self
      .lua
      .registry_value::<Table>(&loaded.local_env)?
      .raw_get_path("<local_env>", &["hive", "start"])?;
    if let Some(f) = start_fn {
      f.call_async(()).await?;
    }
    Ok(())
  }

  pub(crate) async fn run_stop(&self, service: Service) -> Result<()> {
    let loaded = self.load_service(service).await?;
    let stop_fn: Option<Function> = self
      .lua
      .registry_value::<Table>(&loaded.local_env)?
      .raw_get_path("<local_env>", &["hive", "stop"])?;
    if let Some(f) = stop_fn {
      f.call_async(()).await?;
    }
    // Call modules' `stop`
    remove_service_contexts(loaded.service.try_upgrade()?.name());
    Ok(())
  }

  async fn run_source<'a>(
    &'a self,
    name: &str,
    source: Source,
  ) -> Result<(RegistryKey, RegistryKey, Table<'a>)> {
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

  async fn load_service(&self, service: Service) -> Result<Ref<'_, LoadedService>> {
    let mut self_loaded = self.loaded.borrow_mut();
    let service_guard = service.try_upgrade()?;
    let name = service_guard.name();
    if let Some((name_owned, loaded)) = self_loaded.remove_entry(name) {
      if !loaded.service.is_dropped() && loaded.service.ptr_eq(&service) {
        self_loaded.insert(name_owned, loaded);
        drop(self_loaded);
        return Ok(Ref::map(self.loaded.borrow(), |x| x.get(name).unwrap()));
      }
    }
    let source = service_guard.source();
    let (local_env, internal, _) = self.run_source(name, source.clone()).await?;
    let loaded = LoadedService {
      service: service.clone(),
      local_env,
      internal,
    };
    self_loaded.insert(name.into(), loaded);
    drop(self_loaded);
    Ok(Ref::map(self.loaded.borrow(), |x| x.get(name).unwrap()))
  }
}
