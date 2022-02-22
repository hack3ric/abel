use crate::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use hive_vfs::{FileMode, Metadata, Vfs};
use mlua::{ExternalResult, Function, Lua, String as LuaString, Table, UserData};
use std::collections::HashMap;
use std::fmt::Debug;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{RwLock, RwLockReadGuard};

/// Shared, immutable source code storage.
#[derive(Debug, Clone)]
pub struct Source(Arc<SourceInner>);

#[derive(Debug)]
struct SourceInner {
  vfs: Pin<Box<dyn DebugVfs<File = Pin<Box<dyn AsyncRead + Send + Sync>>> + Send + Sync>>,
  cache: RwLock<HashMap<String, Vec<u8>>>,
}

trait DebugVfs: Debug + Vfs {}
impl<T: Debug + Vfs> DebugVfs for T {}

impl Source {
  pub fn new<T>(vfs: T) -> Self
  where
    T: Vfs + Debug + Send + Sync + 'static,
    T::File: Send + Sync + 'static,
  {
    Self(Arc::new(SourceInner {
      vfs: Box::pin(ReadGenericVfs(vfs)),
      cache: Default::default(),
    }))
  }

  pub async fn get<'a>(&'a self, path: &str) -> Result<RwLockReadGuard<'a, [u8]>> {
    let read_guard =
      RwLockReadGuard::try_map(self.0.cache.read().await, |x| x.get(path).map(|x| &x[..]));
    match read_guard {
      Ok(x) => Ok(x),
      Err(_) => {
        let mut f = self.0.vfs.open_file(path, FileMode::Read).await?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).await?;
        drop(read_guard);
        let mut write_guard = self.0.cache.write().await;
        write_guard.insert(path.to_string(), buf);
        drop(write_guard);
        Ok(RwLockReadGuard::map(self.0.cache.read().await, |x| {
          x.get(path).unwrap().as_ref()
        }))
      }
    }
  }

  pub(crate) async fn load<'a>(
    &self,
    lua: &'a Lua,
    path: &str,
    env: Table<'a>,
  ) -> Result<Function<'a>> {
    let code = self.get(path).await?;
    let result = lua
      .load(&*code)
      .set_name(&format!("source:{path}"))?
      .set_environment(env)?
      .into_function()?;
    Ok(result)
  }
}

impl UserData for Source {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method("exists", |_lua, this, path: LuaString| async move {
      let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
      Ok(this.0.vfs.exists(path).await.to_lua_err()?)
    });

    methods.add_async_method(
      "load",
      |lua, this, (path, env): (LuaString, Table)| async move {
        let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
        Ok(this.load(lua, path, env).await.to_lua_err()?)
      },
    )
  }
}

#[derive(Debug)]
struct ReadGenericVfs<T: Vfs + Send + Sync>(T)
where
  T::File: 'static;

#[async_trait]
impl<T: Vfs + Send + Sync> Vfs for ReadGenericVfs<T>
where
  T::File: 'static,
{
  type File = Pin<Box<dyn AsyncRead + Send + Sync>>;

  async fn open_file(&self, path: &str, mode: FileMode) -> hive_vfs::Result<Self::File> {
    Ok(Box::pin(self.0.open_file(path, mode).await?))
  }

  async fn read_dir(&self, path: &str) -> hive_vfs::Result<BoxStream<hive_vfs::Result<String>>> {
    self.0.read_dir(path).await
  }

  async fn metadata(&self, path: &str) -> hive_vfs::Result<Metadata> {
    self.0.metadata(path).await
  }

  async fn exists(&self, path: &str) -> hive_vfs::Result<bool> {
    self.0.exists(path).await
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
