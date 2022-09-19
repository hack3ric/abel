use super::http::LuaUri;
use super::{LuaCacheExt, LUA_HTTP_CLIENT};
use crate::rt_error_fmt;
use anyhow::{anyhow, bail, Context};
use bstr::ByteSlice;
use data_encoding::BASE64URL_NOPAD;
use futures::future::join;
use hyper::body::Bytes;
use hyper::http::uri::{Parts, Scheme};
use hyper::{Body, Response, Uri};
use log::debug;
use mlua::{ExternalResult, Function, Lua, Table, UserData};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::convert::{TryFrom, TryInto};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{read, write, File};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Default)]
pub struct RemoteInterface {
  cache_path: Option<Arc<Path>>,
}

impl RemoteInterface {
  pub fn new(cache_path: Option<PathBuf>) -> Self {
    Self {
      cache_path: cache_path.map(From::from),
    }
  }

  async fn get(&self, path: &str, uri: Uri) -> anyhow::Result<(Bytes, Uri)> {
    let Parts {
      scheme,
      authority,
      path_and_query,
      ..
    } = uri.clone().into_parts();
    let scheme = scheme.unwrap_or(Scheme::HTTP);
    let authority = authority.ok_or_else(|| anyhow!("invalid uri '{uri}' (authority required)"))?;
    let path_and_query =
      path_and_query.ok_or_else(|| anyhow!("invalid uri '{uri}' (path required)"))?;
    let uri_path = path_and_query.path();
    let query = path_and_query
      .query()
      .map(|x| format!("?{x}").into())
      .unwrap_or(Cow::Borrowed(""));

    let segments = uri_path
      .split('/')
      .chain(path.split('.'))
      .filter(|x| !x.is_empty());
    let base_path: String = itertools::intersperse(segments, "/").collect();

    let init_uri = Uri::builder()
      .scheme(scheme.clone())
      .authority(authority.clone())
      .path_and_query(format!("/{base_path}/init.lua{query}"))
      .build()?;
    let file_uri = Uri::builder()
      .scheme(scheme)
      .authority(authority)
      .path_and_query(format!("/{base_path}.lua{query}"))
      .build()?;
    if path.is_empty() {
      debug!("Loading '@{uri}'");
    } else {
      debug!("Loading '{path} @{uri}'");
    }
    let resps = join(request_ok(init_uri.clone()), request_ok(file_uri.clone())).await;

    match resps {
      (Ok((uri, mut resp)), Err(_)) | (Err(_), Ok((uri, mut resp))) => {
        let body = hyper::body::to_bytes(resp.body_mut()).await?;
        debug!("Downloaded {uri}");
        Ok((body, uri))
      }
      (Ok(_), Ok(_)) => bail!("file '{init_uri}' and '{file_uri}' conflicts"),
      (Err(e1), Err(e2)) => bail!(
        "module '{path}' not found\n\
        \tfailed to load '{init_uri}' ({e1})\n\
        \tfailed to load '{file_uri}' ({e2})"
      ),
    }
  }

  async fn get_cached(&self, path: &str, uri: Uri) -> anyhow::Result<(Bytes, Uri)> {
    match self.cache_path.as_deref() {
      Some(cache_path) => {
        let hash = Sha256::new()
          .chain_update(path)
          .chain_update(uri.to_string())
          .finalize_reset();
        let file_name = BASE64URL_NOPAD.encode(&hash);
        let cache_file_path = cache_path.join(file_name);

        if let Ok(mut file) = File::open(&cache_file_path).await {
          if path.is_empty() {
            debug!(
              "Loading cached '@{uri}' from '{}'",
              cache_file_path.display()
            );
          } else {
            debug!(
              "Loading cached '{path} @{uri}' from '{}'",
              cache_file_path.display()
            );
          }
          let mut buf = Vec::with_capacity(file.metadata().await?.len() as _);
          file.read_to_end(&mut buf).await?;
          let metadata = read(cache_file_path.with_extension("metadata")).await?;
          let CacheMetadata { uri } = serde_json::from_slice(&metadata)?;
          Ok((buf.into(), uri.try_into()?))
        } else {
          let (bytes, uri) = self.get(path, uri).await?;
          write(&cache_file_path, &bytes).await?;
          let uri_string = uri.to_string();
          let metadata = serde_json::to_vec(&CacheMetadata { uri: &*uri_string })?;
          write(cache_file_path.with_extension("metadata"), metadata).await?;
          Ok((bytes, uri))
        }
      }
      None => self.get(path, uri).await,
    }
  }
}

async fn request_ok(uri: Uri) -> anyhow::Result<(Uri, Response<Body>)> {
  let resp = LUA_HTTP_CLIENT.get(uri.clone()).await?;
  if resp.status() != 200 {
    bail!("server responded with status code {}", resp.status())
  }
  match resp.headers().get("content-type") {
    Some(ct) => {
      let ct = ct
        .to_str()
        .context("failed to parse content-type as UTF-8")?;
      if !ct.contains("lua") && !ct.starts_with("text/plain") {
        bail!("content type '{ct}' mismatch")
      }
    }
    None => bail!("content-type missing"),
  }
  Ok((uri, resp))
}

impl UserData for RemoteInterface {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method(
      "load",
      |lua, this, (path, uri, env): (mlua::String, mlua::String, Table)| async move {
        let path = path.to_str().map_err(|error| {
          rt_error_fmt!("invalid path '{}' ({error})", path.as_bytes().as_bstr())
        })?;
        let uri = Uri::try_from(uri.as_bytes())
          .map_err(|error| rt_error_fmt!("invalid uri '{}' ({error})", uri.as_bytes().as_bstr()))?;
        this
          .get_cached(path, uri)
          .await
          .to_lua_err()
          .and_then(|(x, uri)| {
            let loader = lua
              .load(&*x)
              .set_environment(env)?
              .set_name(format!("@{uri}"))?
              .into_function()?;
            Ok((loader, LuaUri(uri)))
          })
      },
    );

    methods.add_async_method(
      "get",
      |lua, this, (path, uri): (mlua::String, mlua::String)| async move {
        let path = path.to_str().map_err(|error| {
          rt_error_fmt!("invalid path '{}' ({error})", path.as_bytes().as_bstr())
        })?;
        let uri = Uri::try_from(uri.as_bytes())
          .map_err(|error| rt_error_fmt!("invalid uri '{}' ({error})", uri.as_bytes().as_bstr()))?;
        this
          .get_cached(path, uri)
          .await
          .to_lua_err()
          .and_then(|(bytes, uri)| Ok((lua.create_string(&bytes)?, LuaUri(uri))))
      },
    );
  }
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMetadata<'a> {
  uri: &'a str,
}

pub fn load_create_require(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_value("abel:create_require", || {
    lua
      .load(include_str!("create_require.lua"))
      .set_name("@[create_require]")?
      .into_function()
  })
}
