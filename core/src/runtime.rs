use crate::lua::error::{resolve_callback_error, rt_error_fmt, CustomError};
use crate::lua::http::{LuaRequest, LuaResponse};
use crate::lua::isolate::Isolate;
use crate::lua::sandbox::Sandbox;
use crate::lua::LuaTableExt;
use crate::path::PathMatcher;
use crate::service::{get_local_storage_path, RunningService};
use crate::source::Source;
use crate::task::TaskContext;
use crate::ErrorKind::{self, *};
use crate::{AbelState, Error, Result};
use clru::CLruCache;
use hyper::{Body, Request};
use log::{debug, info};
use mlua::{self, ExternalError, FromLuaMulti, Function, Table, TableExt, ToLuaMulti};
use nonzero_ext::nonzero;
use once_cell::sync::Lazy;
use regex::Regex;
use std::cell::{Ref, RefCell};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub struct Runtime {
  sandbox: Sandbox,
  loaded: RefCell<CLruCache<Box<str>, LoadedService>>,
  state: Arc<AbelState>,
}

#[derive(Debug)]
struct LoadedService {
  service: RunningService,
  isolate: Isolate,
}

impl Runtime {
  pub fn new(state: Arc<AbelState>) -> mlua::Result<Self> {
    let loaded = RefCell::new(CLruCache::new(nonzero!(16usize)));
    let sandbox = Sandbox::new()?;
    Ok(Self {
      sandbox,
      loaded,
      state,
    })
  }

  async fn call_extract_error<'a, T, R>(&'a self, f: mlua::Value<'a>, v: T) -> Result<R>
  where
    T: ToLuaMulti<'a>,
    R: FromLuaMulti<'a>,
  {
    fn sanitize_error(error: mlua::Error) -> Error {
      fn extract_custom_error(
        error: &Arc<dyn std::error::Error + Send + Sync + 'static>,
      ) -> Option<Error> {
        let maybe_custom = error.downcast_ref::<CustomError>();
        maybe_custom.map(|x| ErrorKind::Custom(x.clone()).into())
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

    // `loaded` is a mapped, immutable, checked-at-runtime borrow from
    // `self.loaded`. Dropping it early here prevents `self.loaded` being borrowed
    // more than once at a time.
    let internal = {
      let loaded = self.load_service(service.clone()).await?;
      self.get_internal(&loaded.isolate)?
    };

    for f in internal
      .raw_get_path::<Table>("<internal>", &["paths"])?
      .sequence_values::<Table>()
    {
      let f = f?;
      let path = f.raw_get::<u8, String>(1)?;
      if path == matcher.as_str() {
        let handler = f.raw_get::<u8, mlua::Value>(2)?;

        // Request object in handler should be ephemeral, otherwise graceful shutdown
        // would be blocked.
        let req = self.lua().create_userdata(LuaRequest::new(req, params))?;
        TaskContext::register(self.lua(), req.clone())?;

        let resp = self.call_extract_error(handler, req).await?;
        return Ok(resp);
      }
    }
    unreachable!("path matched but no handler found")
  }

  /// Extracts information from the code, but does not create the service yet
  pub(crate) async fn prepare_service(
    &self,
    name: &str,
    source: Source,
  ) -> Result<(Vec<PathMatcher>, Isolate)> {
    check_name(name)?;
    let (isolate, internal) = self.run_source(name, source).await?;

    let mut paths = Vec::new();
    for f in internal
      .raw_get_path::<Table>("<internal>", &["paths"])?
      .sequence_values::<Table>()
    {
      let path = f?.raw_get::<_, String>(1u8)?;
      let path = PathMatcher::new(&path)?;
      paths.push(path);
    }

    Ok((paths, isolate))
  }

  pub(crate) async fn create_service(
    &self,
    name: &str,
    service: RunningService,
    isolate: Isolate,
    hot_update: bool,
  ) -> Result<()> {
    if service.is_dropped() {
      return Err(ServiceDropped.into());
    }
    let loaded = LoadedService {
      service: service.clone(),
      isolate,
    };
    self.loaded.borrow_mut().put(name.into(), loaded);
    if !hot_update {
      self.run_start(service).await?;
    }
    Ok(())
  }

  pub(crate) async fn run_start(&self, service: RunningService) -> Result<()> {
    // TODO: check validity
    let start_fn: Option<Function> = {
      let loaded = self.load_service(service).await?;
      self
        .get_local_env(&loaded.isolate)?
        .raw_get_path("<local_env>", &["abel", "start"])?
    };
    if let Some(f) = start_fn {
      f.call_async(()).await?;
    }
    Ok(())
  }

  pub(crate) async fn run_stop(&self, service: RunningService) -> Result<()> {
    let stop_fn: Option<Function> = {
      let loaded = self.load_service(service).await?;
      self
        .get_local_env(&loaded.isolate)?
        .raw_get_path("<local_env>", &["abel", "stop"])?
    };
    if let Some(f) = stop_fn {
      f.call_async(()).await?;
    }
    // Call modules' `stop`
    Ok(())
  }

  async fn run_source<'a>(&'a self, name: &str, source: Source) -> Result<(Isolate, Table<'a>)> {
    let local_storage_path = get_local_storage_path(&self.state, name);
    let isolate = self
      .create_isolate(name, source.clone(), local_storage_path)
      .await?;
    self.run_isolate(&isolate, "main.lua", ()).await?;

    let internal = self.get_internal(&isolate)?;
    internal.raw_set("sealed", true)?;

    Ok((isolate, internal))
  }

  async fn load_service(&self, service: RunningService) -> Result<Ref<'_, LoadedService>> {
    let service_guard = service.try_upgrade()?;
    let name = service_guard.name();
    {
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
          self.remove_isolate(loaded.isolate)?;
        }
      }
      debug!(
        "service {name} cache miss on '{}'",
        std::thread::current().name().unwrap_or("<unnamed>")
      );
    }
    let source = service_guard.source();
    let (isolate, _) = self.run_source(name, source.clone()).await?;

    let loaded = LoadedService {
      service: service.clone(),
      isolate,
    };
    let mut self_loaded = self.loaded.borrow_mut();
    self_loaded.put(name.into(), loaded);
    drop(self_loaded);
    self.loaded.borrow_mut().get(name);
    Ok(Ref::map(self.loaded.borrow(), |x| x.peek(name).unwrap()))
  }

  pub fn cleanup(&self) {
    let mut count = 0;
    self.loaded.borrow_mut().retain(|_, v| {
      let r = !v.service.is_dropped();
      if !r {
        count += 1;
      }
      r
    });
    if count > 0 {
      info!("successfully cleaned {count} dropped services");
    }
  }
}

impl Deref for Runtime {
  type Target = Sandbox;

  fn deref(&self) -> &Self::Target {
    &self.sandbox
  }
}

impl DerefMut for Runtime {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.sandbox
  }
}

pub fn check_name(name: &str) -> Result<()> {
  static NAME_CHECK_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new("^[a-z0-9-]{1,64}$").unwrap());

  if NAME_CHECK_REGEX.is_match(name) {
    Ok(())
  } else {
    Err(InvalidServiceName { name: name.into() }.into())
  }
}
