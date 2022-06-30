use super::global_env::modify_global_env;
use super::http::LuaResponse;
use super::local_env::create_local_env;
use super::LuaTableExt;
use crate::lua::error::rt_error_fmt;
use crate::lua::http::LuaRequest;
use crate::path::PathMatcher;
use crate::service::RunningService;
use crate::source::Source;
use crate::ErrorKind::{self, *};
use crate::{Error, HiveState, Result};
use clru::CLruCache;
use hyper::{Body, Request};
use log::debug;
use mlua::{ExternalError, FromLuaMulti, Function, Lua, RegistryKey, Table, TableExt, ToLuaMulti};
use nonzero_ext::nonzero;
use once_cell::sync::Lazy;
use regex::Regex;
use std::cell::{Ref, RefCell};
use std::sync::Arc;

static NAME_CHECK_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new("^[a-z0-9-]{1,64}$").unwrap());

#[derive(Debug)]
pub struct Sandbox {
  pub(crate) lua: Lua,
  loaded: RefCell<CLruCache<Box<str>, LoadedService>>,
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
    let loaded = RefCell::new(CLruCache::new(nonzero!(16usize)));
    modify_global_env(&lua)?;
    Ok(Self { lua, loaded, state })
  }

  async fn call_extract_error<'a, T, R>(&'a self, f: mlua::Value<'a>, v: T) -> Result<R>
  where
    T: ToLuaMulti<'a>,
    R: FromLuaMulti<'a>,
  {
    fn sanitize_error(error: mlua::Error) -> Error {
      fn resolve_callback_error(error: &mlua::Error) -> &mlua::Error {
        match error {
          mlua::Error::CallbackError {
            traceback: _,
            cause,
          } => resolve_callback_error(cause),
          _ => error,
        }
      }

      fn extract_custom_error(
        error: &Arc<dyn std::error::Error + Send + Sync + 'static>,
      ) -> Option<Error> {
        let maybe_custom = error.downcast_ref::<Error>().map(Error::kind);
        if let Some(ErrorKind::Custom {
          status,
          error,
          detail,
        }) = maybe_custom
        {
          Some(From::from(ErrorKind::Custom {
            status: *status,
            error: error.clone(),
            detail: detail.clone(),
          }))
        } else {
          None
        }
      }

      match error {
        mlua::Error::CallbackError { traceback, cause } => {
          let cause = resolve_callback_error(&cause);
          if let mlua::Error::ExternalError(error) = cause {
            if let Some(error) = extract_custom_error(error) {
              return error;
            }
          }
          format!("{cause}\n{traceback}").to_lua_err().into()
        }
        mlua::Error::ExternalError(error) => {
          extract_custom_error(&error).unwrap_or_else(|| mlua::Error::ExternalError(error).into())
        }
        _ => error.into(),
      }
    }

    let result = match f {
      mlua::Value::Function(f) => f.call_async(v).await,
      mlua::Value::Table(f) => f.call_async(v).await,
      _ => return Err(rt_error_fmt!("attempt to call a(n) {} value", f.type_name()).into()),
    };
    result.map_err(sanitize_error)
  }

  pub async fn handle_request(
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
        let handler = f.raw_get::<u8, mlua::Value>(2)?;
        let req = LuaRequest::new(req, params);
        let resp = self.call_extract_error(handler, req).await?;
        return Ok(resp);
      }
    }
    panic!("path matched but no handler found; this is a bug")
  }

  /// Extracts information from the code, but does not create the service yet
  pub(crate) async fn prepare_service(
    &self,
    name: &str,
    source: Source,
  ) -> Result<(Vec<PathMatcher>, RegistryKey, RegistryKey)> {
    if !NAME_CHECK_REGEX.is_match(name) {
      return Err(InvalidServiceName { name: name.into() }.into());
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

  pub(crate) async fn create_service(
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
    self.loaded.borrow_mut().put(name.into(), loaded);
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
    Ok(())
  }

  async fn run_source<'a>(
    &'a self,
    name: &str,
    source: Source,
  ) -> Result<(RegistryKey, RegistryKey, Table<'a>)> {
    let (local_env, internal) =
      create_local_env(&self.lua, &self.state, name, source.clone()).await?;
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
    if let Some(loaded) = self_loaded.pop(name) {
      if !loaded.service.is_dropped() && loaded.service.ptr_eq(&service) {
        debug!(
          "service {name} cache hit on '{}'",
          std::thread::current().name().unwrap_or("<unnamed>")
        );
        self_loaded.put(name.into(), loaded);
        drop(self_loaded);
        self.loaded.borrow_mut().get(name);
        return Ok(Ref::map(self.loaded.borrow(), |x| x.peek(name).unwrap()));
      } else {
        self.lua.remove_registry_value(loaded.internal)?;
        self.lua.remove_registry_value(loaded.local_env)?;
      }
    }
    debug!(
      "service {name} cache miss on '{}'",
      std::thread::current().name().unwrap_or("<unnamed>")
    );
    drop(self_loaded);
    let source = service_guard.source();
    let (local_env, internal, _) = self.run_source(name, source.clone()).await?;

    let loaded = LoadedService {
      service: service.clone(),
      local_env,
      internal,
    };
    let mut self_loaded = self.loaded.borrow_mut();
    self_loaded.put(name.into(), loaded);
    drop(self_loaded);
    self.loaded.borrow_mut().get(name);
    Ok(Ref::map(self.loaded.borrow(), |x| x.peek(name).unwrap()))
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
