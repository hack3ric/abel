use super::LUA_HTTP_CLIENT;
use crate::rt_error_fmt;
use anyhow::{anyhow, bail, Context};
use bstr::ByteSlice;
use futures::future::join;
use hyper::body::Bytes;
use hyper::http::uri::{Parts, Scheme};
use hyper::{Body, Response, Uri};
use mlua::{ExternalResult, UserData};
use std::borrow::Cow;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

  // TODO: cache
  async fn get(&self, path: &str, uri: Uri) -> anyhow::Result<(Uri, Bytes)> {
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
    let base_path = path.replace('.', "/");

    let init_uri = Uri::builder()
      .scheme(scheme.clone())
      .authority(authority.clone())
      .path_and_query(format!("{uri_path}/{base_path}/init.lua{query}"))
      .build()?;
    let file_uri = Uri::builder()
      .scheme(scheme)
      .authority(authority)
      .path_and_query(format!("{uri_path}/{base_path}.lua{query}"))
      .build()?;
    let resps = join(request_ok(init_uri.clone()), request_ok(file_uri.clone())).await;

    match resps {
      (Ok((uri, mut resp)), Err(_)) | (Err(_), Ok((uri, mut resp))) => {
        Ok((uri, hyper::body::to_bytes(resp.body_mut()).await?))
      }
      (Ok(_), Ok(_)) => bail!("file '{init_uri}' and '{file_uri}' conflicts"),
      (Err(e1), Err(e2)) => bail!(
        "module '{path}' not found\n\
        \tfailed to load '{init_uri}' ({e1})\n\
        \tfailed to load '{file_uri}' ({e2})"
      ),
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
      if !ct.contains("lua") {
        bail!("content type '{ct}' does not contain 'lua'")
      }
    }
    None => bail!("content-type missing"),
  }
  Ok((uri, resp))
}

impl UserData for RemoteInterface {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method(
      "get",
      |lua, this, (path, uri): (mlua::String, mlua::String)| async move {
        let path = path.to_str().map_err(|error| {
          rt_error_fmt!("invalid path '{}' ({error})", path.as_bytes().as_bstr())
        })?;
        let uri = Uri::try_from(uri.as_bytes())
          .map_err(|error| rt_error_fmt!("invalid uri '{}' ({error})", uri.as_bytes().as_bstr()))?;
        this
          .get(path, uri)
          .await
          .to_lua_err()
          .and_then(|(uri, x)| Ok((uri.to_string(), lua.create_string(&x)?)))
      },
    )
  }
}
