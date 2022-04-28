use crate::path::normalize_path_str;
use crate::Result;
use async_trait::async_trait;
use mlua::{ExternalResult, Function, Lua, Table, UserData};
use std::fs::Metadata;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::fs::{canonicalize, rename, File};
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncSeek, AsyncWrite};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct DirSource {
  base: Arc<RwLock<PathBuf>>,
}

impl DirSource {
  pub async fn new(base: impl AsRef<Path>) -> Result<Self> {
    let base = canonicalize(base).await?;
    Ok(Self {
      base: Arc::new(RwLock::new(base)),
    })
  }

  pub async fn rename_base(&self, new_path: PathBuf) -> Result<()> {
    let mut base = self.base.write().await;
    rename(&*base, &new_path).await?;
    *base = new_path;
    Ok(())
  }
}

#[async_trait(?Send)]
impl Source for DirSource {
  async fn get(&self, path: &str) -> io::Result<GenericFile> {
    let path = normalize_path_str(path);
    let file = File::open(self.base.read().await.join(path)).await?;
    Ok(Box::pin(file))
  }

  async fn exists(&self, path: &str) -> bool {
    (self.base.read().await)
      .join(normalize_path_str(path))
      .exists()
  }
}

// TODO: impl UserData for any type that implements Source
impl UserData for DirSource {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method("exists", |_lua, this, path: mlua::String| async move {
      let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
      Ok(this.exists(path).await)
    });

    methods.add_async_method(
      "load",
      |lua, this, (path, env): (mlua::String, Table)| async move {
        let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
        this.load(lua, path, env).await.to_lua_err()
      },
    )
  }
}

#[derive(Debug, Clone)]
pub struct DummySource {
  main: Arc<[u8]>,
}

#[async_trait(?Send)]
impl Source for DummySource {
  async fn get(&self, path: &str) -> io::Result<GenericFile> {
    if normalize_path_str(path) == "main.lua" {
      let pseudo_file = Cursor::new(self.main.clone());
      Ok(Box::pin(ReadOnlyArcCursor(pseudo_file)))
    } else {
      Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("file {path} not found"),
      ))
    }
  }

  async fn exists(&self, path: &str) -> bool {
    normalize_path_str(path) == "main.lua"
  }
}

struct ReadOnlyArcCursor(Cursor<Arc<[u8]>>);

impl AsyncRead for ReadOnlyArcCursor {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut io::ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    Pin::new(&mut self.0).poll_read(cx, buf)
  }
}

impl AsyncWrite for ReadOnlyArcCursor {
  fn poll_write(
    self: Pin<&mut Self>,
    _cx: &mut Context<'_>,
    _buf: &[u8],
  ) -> Poll<Result<usize, io::Error>> {
    // TODO: better error message?
    Poll::Ready(Err(io::Error::new(
      io::ErrorKind::Other,
      "bad file descriptor",
    )))
  }

  fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
    Poll::Ready(Ok(()))
  }

  fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
    Poll::Ready(Ok(()))
  }
}

impl AsyncSeek for ReadOnlyArcCursor {
  fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> std::io::Result<()> {
    Pin::new(&mut self.0).start_seek(position)
  }

  fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
    Pin::new(&mut self.0).poll_complete(cx)
  }
}

#[async_trait(?Send)]
impl FileLike for ReadOnlyArcCursor {
  async fn metadata(&self) -> io::Result<Metadata> {
    // TODO: metadata for cursor
    todo!()
  }
}

#[async_trait(?Send)]
pub trait Source {
  async fn get(&self, path: &str) -> io::Result<GenericFile>;

  async fn get_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
    let mut code_file = self.get(path).await?;
    let mut code = if let Ok(metadata) = code_file.metadata().await {
      Vec::with_capacity(metadata.len() as _)
    } else {
      Vec::new()
    };
    code_file.read_to_end(&mut code).await?;
    Ok(code)
  }

  async fn exists(&self, path: &str) -> bool;

  async fn load<'a>(&self, lua: &'a Lua, path: &str, env: Table<'a>) -> Result<Function<'a>> {
    let code = self.get_bytes(path).await?;
    let result = lua
      .load(&code)
      .set_name(&format!("source:{path}"))?
      .set_environment(env)?
      .into_function()?;
    Ok(result)
  }
}

pub type GenericFile = Pin<Box<dyn FileLike + Send>>;

#[async_trait(?Send)]
pub trait FileLike: AsyncRead + AsyncWrite + AsyncSeek {
  async fn metadata(&self) -> io::Result<Metadata>;
}

#[async_trait(?Send)]
impl FileLike for File {
  async fn metadata(&self) -> io::Result<Metadata> {
    self.metadata().await
  }
}
