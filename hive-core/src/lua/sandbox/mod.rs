mod global_env;
mod local_env;

use super::http::LuaResponse;
use super::shared::remove_service_shared_stores;
use super::LuaTableExt;
use crate::lua::http::LuaRequest;
use crate::path::PathMatcher;
use crate::permission::PermissionSet;
use crate::service::RunningService;
use crate::source::Source;
use crate::ErrorKind::*;
use crate::{HiveState, Result};
use global_env::modify_global_env;
use hyper::{Body, Request};
use local_env::create_local_env;
use mlua::{
  ExternalResult, FromLuaMulti, Function, Lua, LuaSerdeExt, MultiValue, RegistryKey, Table,
  ToLuaMulti,
};
use once_cell::sync::Lazy;
use regex::Regex;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

static NAME_CHECK_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new("^[a-z0-9-]{1,64}$").unwrap());

#[derive(Debug)]
pub struct Sandbox {
  lua: Lua,
  loaded: RefCell<HashMap<Box<str>, LoadedService>>,
  state: Arc<HiveState>,
}

#[derive(Debug)]
struct LoadedService {
  service: RunningService,
  local_env: RegistryKey,
  internal: RegistryKey,
}

impl Sandbox {
  pub fn new(state: Arc<HiveState>) -> Result<Self> {
    let lua = Lua::new();
    let loaded = RefCell::new(HashMap::new());
    modify_global_env(&lua)?;
    Ok(Self { lua, loaded, state })
  }
}

impl Sandbox {
  async fn pcall<'a, T, R>(&'a self, f: Function<'a>, v: T) -> Result<R>
  where
    T: ToLuaMulti<'a>,
    R: FromLuaMulti<'a>,
  {
    let pcall: Function = self.lua.globals().raw_get("pcall")?;
    let error: Function = self.lua.globals().raw_get("error")?;
    let (succeeded, obj) = pcall.call_async::<_, (bool, MultiValue)>((f, v)).await?;
    if succeeded {
      Ok(FromLuaMulti::from_lua_multi(obj, &self.lua)?)
    } else {
      let error_obj = obj.into_vec().remove(0);
      if let mlua::Value::Table(custom_error) = error_obj {
        Err(
          crate::ErrorKind::LuaCustom {
            status: custom_error
              .raw_get::<_, u16>("status")?
              .try_into()
              .to_lua_err()?,
            error: std::str::from_utf8(
              custom_error.raw_get::<_, mlua::String>("error")?.as_bytes(),
            )
            .to_lua_err()?
            .into(),
            detail: (self.lua).from_value(custom_error.raw_get::<_, mlua::Value>("detail")?)?,
          }
          .into(),
        )
      } else {
        error.call(error_obj)?;
        unreachable!()
      }
    }
  }

  pub async fn run(
    &self,
    service: RunningService,
    path: &str,
    req: Request<Body>,
  ) -> Result<LuaResponse> {
    let guard = service.try_upgrade()?;
    let (params, matcher) = guard
      .paths()
      .iter()
      .find_map(|m| m.gen_params(path).map(|p| (p, m)))
      .ok_or_else(|| ServicePathNotFound {
        service: guard.name().into(),
        path: path.into(),
      })?;

    let loaded = self.load_service(service.clone()).await?;
    let internal: Table = self.lua.registry_value(&loaded.internal)?;

    // `loaded` is a mapped, immutable, checked-at-runtime borrow from
    // `self.loaded`. Dropping it manually here prevents `self.loaded` being
    // borrowed more than once at a time.
    drop(loaded);

    for f in internal
      .raw_get_path::<Table>("<internal>", &["paths"])?
      .sequence_values::<Table>()
    {
      let f = f?;
      let path = f.raw_get::<u8, String>(1)?;
      if path == matcher.as_str() {
        let handler = f.raw_get::<u8, Function>(2)?;
        let req = LuaRequest::new(req, params);
        let resp = self.pcall(handler, req).await?;
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
    permissions: Arc<PermissionSet>,
  ) -> Result<(Vec<PathMatcher>, RegistryKey, RegistryKey)> {
    if !NAME_CHECK_REGEX.is_match(name) {
      return Err(InvalidServiceName { name: name.into() }.into());
    }

    let (local_env, internal_key, internal) = self.run_source(name, source, permissions).await?;

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
    service: RunningService,
    local_env: RegistryKey,
    internal: RegistryKey,
    hot_update: bool,
  ) -> Result<()> {
    if service.is_dropped() {
      return Err(ServiceDropped.into());
    }
    let loaded = LoadedService {
      service: service.clone(),
      local_env,
      internal,
    };
    self.loaded.borrow_mut().insert(name.into(), loaded);
    if !hot_update {
      self.run_start(service).await?;
    }
    Ok(())
  }

  pub(crate) fn remove_registry(&self, key: RegistryKey) -> mlua::Result<()> {
    self.lua.remove_registry_value(key)
  }

  pub(crate) fn expire_registry_values(&self) {
    self.lua.expire_registry_values();
  }

  pub(crate) async fn run_start(&self, service: RunningService) -> Result<()> {
    let loaded = self.load_service(service).await?;
    let start_fn: Option<Function> = (self.lua)
      .registry_value::<Table>(&loaded.local_env)?
      .raw_get_path("<local_env>", &["hive", "start"])?;
    if let Some(f) = start_fn {
      f.call_async(()).await?;
    }
    Ok(())
  }

  pub(crate) async fn run_stop(&self, service: RunningService) -> Result<()> {
    let loaded = self.load_service(service).await?;
    let stop_fn: Option<Function> = (self.lua)
      .registry_value::<Table>(&loaded.local_env)?
      .raw_get_path("<local_env>", &["hive", "stop"])?;
    if let Some(f) = stop_fn {
      f.call_async(()).await?;
    }
    // Call modules' `stop`
    let service = loaded.service.try_upgrade()?;
    let service_name = service.name();
    remove_service_shared_stores(service_name);
    // if destroy {
    //   remove_service_local_storage(&self.state, service_name).await?;
    // }
    Ok(())
  }

  async fn run_source<'a>(
    &'a self,
    name: &str,
    source: Source,
    permissions: Arc<PermissionSet>,
  ) -> Result<(RegistryKey, RegistryKey, Table<'a>)> {
    let (local_env, internal) =
      create_local_env(&self.lua, &self.state, name, source.clone(), permissions).await?;
    source
      .load(&self.lua, "/main.lua", local_env.clone())
      .await?
      .call_async::<_, ()>(())
      .await?;
    internal.raw_set("sealed", true)?;
    let local_env_key = self.lua.create_registry_value(local_env)?;
    let internal_key = self.lua.create_registry_value(internal.clone())?;
    Ok((local_env_key, internal_key, internal))
  }

  async fn load_service(&self, service: RunningService) -> Result<Ref<'_, LoadedService>> {
    let service_guard = service.try_upgrade()?;
    let name = service_guard.name();
    let mut self_loaded = self.loaded.borrow_mut();
    if let Some((name_owned, loaded)) = self_loaded.remove_entry(name) {
      if !loaded.service.is_dropped() && loaded.service.ptr_eq(&service) {
        self_loaded.insert(name_owned, loaded);
        drop(self_loaded);
        return Ok(Ref::map(self.loaded.borrow(), |x| x.get(name).unwrap()));
      } else {
        self.lua.remove_registry_value(loaded.internal)?;
        self.lua.remove_registry_value(loaded.local_env)?;
      }
    }
    drop(self_loaded);
    let source = service_guard.source();
    let (local_env, internal, _) = self
      .run_source(name, source.clone(), service_guard.permissions_arc())
      .await?;

    let loaded = LoadedService {
      service: service.clone(),
      local_env,
      internal,
    };
    let mut self_loaded = self.loaded.borrow_mut();
    self_loaded.insert(name.into(), loaded);
    drop(self_loaded);
    Ok(Ref::map(self.loaded.borrow(), |x| x.get(name).unwrap()))
  }

  pub(crate) async fn clean_loaded(&self) -> u32 {
    self.lua.expire_registry_values();
    let mut x = self.loaded.borrow_mut();
    let mut count = 0;
    x.retain(|_, v| {
      let r = !v.service.is_dropped();
      if !r {
        count += 1;
      }
      r
    });
    count
  }
}
