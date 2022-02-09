use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use hive_vfs::{normalize_path, FileMode, Metadata, Vfs};
use hyper::{Body, Response, StatusCode};
use serde::Serialize;
use std::fmt::Debug;
use std::io::Cursor;
use std::sync::Arc;

pub fn json_response(status: StatusCode, body: impl Serialize) -> Response<Body> {
  Response::builder()
    .status(status)
    .header("Content-Type", "application/json")
    .body(serde_json::to_string(&body).unwrap().into())
    .unwrap()
}

pub struct SingleMainLua(Arc<[u8]>);

impl SingleMainLua {
  pub fn from_slice(content: impl AsRef<[u8]>) -> Self {
    Self(Arc::from(content.as_ref()))
  }
}

impl Debug for SingleMainLua {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("SingleMainLua(<content>)")
  }
}

#[async_trait]
impl Vfs for SingleMainLua {
  type File = Cursor<Arc<[u8]>>;

  async fn open_file(&self, path: &str, mode: FileMode) -> hive_vfs::Result<Self::File> {
    if mode != FileMode::Read {
      return Err(hive_vfs::Error::MethodNotAllowed);
    }
    if normalize_path(path) == "main.lua" {
      Ok(Cursor::new(self.0.clone()))
    } else {
      Err(hive_vfs::Error::NotFound(path.into()))
    }
  }

  async fn read_dir(&self, path: &str) -> hive_vfs::Result<BoxStream<hive_vfs::Result<String>>> {
    if normalize_path(path).is_empty() {
      Ok(stream::once(async { Ok("/main.lua".to_string()) }).boxed())
    } else {
      Err(hive_vfs::Error::NotFound(path.into()))
    }
  }

  async fn metadata(&self, path: &str) -> hive_vfs::Result<Metadata> {
    if normalize_path(path) == "main.lua" {
      Ok(Metadata::File {
        len: self.0.len() as _,
      })
    } else {
      Err(hive_vfs::Error::NotFound(path.into()))
    }
  }

  async fn exists(&self, path: &str) -> hive_vfs::Result<bool> {
    if normalize_path(path) == "main.lua" {
      Ok(true)
    } else {
      Ok(false)
    }
  }

  async fn create_dir(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }

  async fn remove_file(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }
}
