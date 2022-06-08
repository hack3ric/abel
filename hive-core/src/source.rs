use crate::path::normalize_path_str;
use crate::Result;
use mlua::{ExternalResult, Function, Lua, Table, UserData};
use pin_project::pin_project;
use std::fs::Metadata as FsMetadata;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::fs::{canonicalize, rename, File};
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncSeek, AsyncWrite};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub enum Source {
  Dir(DirSource),
  Dummy(DummySource),
}

impl Source {
  pub async fn get(&self, path: &str) -> io::Result<GenericFile> {
    match self {
      Self::Dir(source) => source.get(path).await,
      Self::Dummy(source) => {
        if normalize_path_str(path) == "main.lua" {
          let pseudo_file = Cursor::new(source.main.clone());
          Ok(GenericFile::ReadOnlyArcCursor(pseudo_file))
        } else {
          Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("file {path} not found"),
          ))
        }
      }
    }
  }

  pub async fn exists(&self, path: &str) -> bool {
    match self {
      Self::Dir(source) => (source.base.read().await)
        .join(normalize_path_str(path))
        .exists(),
      Self::Dummy(_) => normalize_path_str(path) == "main.lua",
    }
  }

  pub async fn get_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
    let mut code_file = self.get(path).await?;
    let mut code = if let Ok(metadata) = code_file.metadata().await {
      Vec::with_capacity(metadata.len() as _)
    } else {
      Vec::new()
    };
    code_file.read_to_end(&mut code).await?;
    Ok(code)
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

impl From<DirSource> for Source {
  fn from(x: DirSource) -> Self {
    Self::Dir(x)
  }
}

impl From<DummySource> for Source {
  fn from(x: DummySource) -> Self {
    Self::Dummy(x)
  }
}

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

  pub async fn get(&self, path: &str) -> io::Result<GenericFile> {
    let path = normalize_path_str(path);
    let file = File::open(self.base.read().await.join(path)).await?;
    Ok(GenericFile::File(file))
  }
}

#[derive(Debug, Clone)]
pub struct DummySource {
  main: Arc<[u8]>,
}

impl DummySource {
  pub fn new(source: impl Into<Arc<[u8]>>) -> Self {
    Self {
      main: source.into(),
    }
  }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceUserData(pub(crate) Source);

// TODO: impl UserData for any type that implements Source
impl UserData for SourceUserData {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method("exists", |_lua, this, path: mlua::String| async move {
      let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
      Ok(this.0.exists(path).await)
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

#[pin_project(project = GenericFileProj)]
pub enum GenericFile {
  File(#[pin] File),
  ReadOnlyArcCursor(#[pin] Cursor<Arc<[u8]>>),
}

impl GenericFile {
  pub async fn metadata(&self) -> io::Result<Metadata> {
    match self {
      Self::File(f) => Ok(f.metadata().await?.into()),
      Self::ReadOnlyArcCursor(f) => Ok(Metadata {
        len: f.get_ref().len() as _,
      }),
    }
  }
}

impl AsyncRead for GenericFile {
  fn poll_read(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut io::ReadBuf<'_>,
  ) -> Poll<std::io::Result<()>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_read(cx, buf),
      GenericFileProj::ReadOnlyArcCursor(f) => f.poll_read(cx, buf),
    }
  }
}

impl AsyncWrite for GenericFile {
  fn poll_write(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &[u8],
  ) -> Poll<Result<usize, std::io::Error>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_write(cx, buf),
      GenericFileProj::ReadOnlyArcCursor(_) => Poll::Ready(Err(io::Error::new(
        io::ErrorKind::Other,
        "bad file descriptor",
      ))),
    }
  }

  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_flush(cx),
      GenericFileProj::ReadOnlyArcCursor(_) => Poll::Ready(Err(io::Error::new(
        io::ErrorKind::Other,
        "bad file descriptor",
      ))),
    }
  }

  fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_shutdown(cx),
      GenericFileProj::ReadOnlyArcCursor(_) => Poll::Ready(Err(io::Error::new(
        io::ErrorKind::Other,
        "bad file descriptor",
      ))),
    }
  }
}

impl AsyncSeek for GenericFile {
  fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> std::io::Result<()> {
    match self.project() {
      GenericFileProj::File(f) => f.start_seek(position),
      GenericFileProj::ReadOnlyArcCursor(f) => f.start_seek(position),
    }
  }

  fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_complete(cx),
      GenericFileProj::ReadOnlyArcCursor(f) => f.poll_complete(cx),
    }
  }
}

#[derive(Debug, Clone)]
pub struct Metadata {
  len: u64,
}

impl Metadata {
  #[allow(clippy::len_without_is_empty)]
  pub fn len(&self) -> u64 {
    self.len
  }
}

impl From<FsMetadata> for Metadata {
  fn from(x: FsMetadata) -> Self {
    Self { len: x.len() }
  }
}
