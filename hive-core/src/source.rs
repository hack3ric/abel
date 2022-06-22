use crate::Result;
use async_trait::async_trait;
use mlua::{ExternalResult, Function, Lua, Table, UserData};
use std::fmt::Debug;
use std::io::SeekFrom;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};

#[async_trait]
pub trait SourceVfs {
  type File: AsyncRead + AsyncSeek;
  async fn get(&self, path: &str) -> io::Result<Self::File>;
  async fn exists(&self, path: &str) -> io::Result<bool>;
}

pub trait AsyncReadSeek: AsyncRead + AsyncSeek {}
impl<T: AsyncRead + AsyncSeek> AsyncReadSeek for T {}

pub type ReadOnlyFile = Pin<Box<dyn AsyncReadSeek + Send + Sync>>;

struct SourceInner<V>(V)
where
  V: SourceVfs + Send + Sync,
  V::File: Send + Sync + 'static;

#[async_trait]
impl<V> SourceVfs for SourceInner<V>
where
  V: SourceVfs + Send + Sync,
  V::File: Send + Sync + 'static,
{
  type File = ReadOnlyFile;

  async fn get(&self, path: &str) -> io::Result<Self::File> {
    let file = self.0.get(path).await?;
    Ok(Box::pin(file) as _)
  }

  async fn exists(&self, path: &str) -> io::Result<bool> {
    self.0.exists(path).await
  }
}

#[derive(Clone)]
pub struct Source(Arc<dyn SourceVfs<File = ReadOnlyFile> + Send + Sync>);

impl Source {
  pub fn new<V>(vfs: V) -> Self
  where
    V: SourceVfs + Send + Sync + 'static,
    V::File: Send + Sync + 'static,
  {
    Self(Arc::new(SourceInner(vfs)) as _)
  }

  async fn get_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
    let mut file = self.get(path).await?;
    let len = file.seek(SeekFrom::End(0)).await?;
    file.rewind().await?;
    let mut buf = Vec::with_capacity(len as _);
    file.read_to_end(&mut buf).await?;
    Ok(buf)
  }

  pub async fn load<'a, 'b>(
    &self,
    lua: &'a Lua,
    path: &'b str,
    env: Table<'a>,
  ) -> Result<Function<'a>> {
    let code = self.get_bytes(path).await?;
    let result = lua
      .load(&code)
      .set_name(&format!("source:{path}"))?
      .set_environment(env)?
      .into_function()?;
    Ok(result)
  }
}

impl Deref for Source {
  type Target = dyn SourceVfs<File = ReadOnlyFile> + Send + Sync;

  fn deref(&self) -> &Self::Target {
    &*self.0
  }
}

impl Debug for Source {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("Source { ... }")
  }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceUserData(pub(crate) Source);

// TODO: impl UserData for any type that implements Source
impl UserData for SourceUserData {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method("exists", |_lua, this, path: mlua::String| async move {
      let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
      this.0.exists(path).await.to_lua_err()
    });

    methods.add_async_method(
      "load",
      |lua, this, (path, env): (mlua::String, Table)| async move {
        let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
        this.0.load(lua, path, env).await.to_lua_err()
      },
    )
  }
}
