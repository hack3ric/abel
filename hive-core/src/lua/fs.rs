use super::error::{arg_error, check_arg, check_userdata_mut, rt_error, tag_error};
use crate::lua::byte_stream::ByteStream;
use crate::lua::context;
use crate::lua::error::rt_error_fmt;
use crate::path::normalize_path_str;
use crate::source::{ReadOnlyFile, Source};
use bstr::ByteSlice;
use mlua::Value::Nil;
use mlua::{
  AnyUserData, ExternalResult, Function, Lua, MultiValue, ToLua, UserData, UserDataMethods,
};
use pin_project::pin_project;
use std::borrow::Cow;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{
  self, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite,
  AsyncWriteExt, BufReader,
};

pub async fn create_preload_fs(
  lua: &Lua,
  local_storage_path: impl Into<PathBuf>,
  source: Source,
) -> mlua::Result<Function<'_>> {
  let local_storage_path: Arc<Path> = local_storage_path.into().into();
  if !local_storage_path.exists() {
    tokio::fs::create_dir(&local_storage_path).await?;
  }
  _create_preload_fs(lua, local_storage_path, source)
}

fn _create_preload_fs(
  lua: &Lua,
  local_storage_path: Arc<Path>,
  source: Source,
) -> mlua::Result<Function<'_>> {
  lua.create_function(move |lua, ()| {
    let fs_table = lua.create_table()?;
    fs_table.raw_set(
      "open",
      create_fn_fs_open(lua, source.clone(), local_storage_path.clone())?,
    )?;
    fs_table.raw_set(
      "mkdir",
      create_fn_fs_mkdir(lua, local_storage_path.clone())?,
    )?;
    fs_table.raw_set(
      "remove",
      create_fn_fs_remove(lua, local_storage_path.clone())?,
    )?;
    Ok(fs_table)
  })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenMode {
  Read,
  Write,
  Append,
  ReadWrite,
  ReadWriteNew,
  ReadAppend,
}

impl OpenMode {
  fn from_lua(mode: Option<mlua::String>) -> mlua::Result<Self> {
    use OpenMode::*;
    if let Some(mode) = mode {
      let result = match mode.as_bytes() {
        b"r" => Read,
        b"w" => Write,
        b"a" => Append,
        b"r+" => ReadWrite,
        b"w+" => ReadWriteNew,
        b"a+" => ReadAppend,
        mode => return Err(rt_error_fmt!("invalid open mode: {}", mode.as_bstr())),
      };
      Ok(result)
    } else {
      Ok(Self::Read)
    }
  }

  fn to_open_options(self) -> OpenOptions {
    use OpenMode::*;
    let mut options = OpenOptions::new();
    match self {
      Read => options.read(true),
      Write => options.create(true).truncate(true).write(true),
      Append => options.create(true).append(true),
      ReadWrite => options.read(true).write(true),
      ReadWriteNew => options.create(true).truncate(true).read(true).write(true),
      ReadAppend => options.create(true).read(true).append(true),
    };
    options
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadMode {
  All,
  Exact(u64),
  Line,
  LineWithDelimiter,
  // Numeral,
}

impl ReadMode {
  fn from_lua(mode: mlua::Value) -> mlua::Result<Self> {
    match mode {
      mlua::Value::Integer(i) if i > 0 => Ok(Self::Exact(i as _)),
      mlua::Value::Integer(_) => Err(rt_error("read bytes cannot be negative")),
      mlua::Value::String(s) => match s.as_bytes() {
        b"a" => Ok(Self::All),
        b"l" => Ok(Self::Line),
        b"L" => Ok(Self::LineWithDelimiter),
        s => Err(rt_error_fmt!("invalid file read mode {:?}", s.as_bstr())),
      },
      _ => Err(rt_error_fmt!(
        "string or integer expected, got {}",
        mode.type_name()
      )),
    }
  }
}

pub struct LuaFile(BufReader<GenericFile>);

async fn read_once<'lua>(
  this: &mut LuaFile,
  lua: &'lua Lua,
  mode: ReadMode,
) -> mlua::Result<mlua::Value<'lua>> {
  use ReadMode::*;
  match mode {
    All => {
      let file_ref = this.0.get_mut();
      let file_len = file_ref.len().await?;
      let pos = file_ref.seek(SeekFrom::Current(0)).await?;
      let len = file_len - pos;
      let mut buf = Vec::with_capacity(len as _);
      this.0.read_to_end(&mut buf).await?;
      Ok(mlua::Value::String(lua.create_string(&buf)?))
    }
    Exact(len) => {
      if len == 0 {
        "".to_lua(lua)
      } else {
        let len = len.min(this.0.get_mut().len().await?);
        let mut buf = vec![0; len as _];
        let actual_len = this.0.read_exact(&mut buf).await?;
        if actual_len == 0 {
          Ok(Nil)
        } else {
          buf.truncate(actual_len);
          Ok(mlua::Value::String(lua.create_string(&buf)?))
        }
      }
    }
    Line => {
      let mut buf = String::new();
      let bytes = this.0.read_line(&mut buf).await?;
      if bytes == 0 {
        Ok(Nil)
      } else {
        if buf.ends_with('\n') {
          buf.pop();
        }
        if buf.ends_with('\r') {
          buf.pop();
        }
        Ok(mlua::Value::String(lua.create_string(&buf)?))
      }
    }
    LineWithDelimiter => {
      let mut buf = String::new();
      let bytes = this.0.read_line(&mut buf).await?;
      if bytes == 0 {
        Ok(Nil)
      } else {
        Ok(mlua::Value::String(lua.create_string(&buf)?))
      }
    }
  }
}

impl UserData for LuaFile {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_function("__close", |_lua, this: AnyUserData| {
      drop(this.take::<Self>());
      Ok(())
    });

    methods.add_async_function("read", |lua, args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "file", 1)?;
      let modes = args.iter().skip(1);
      let mut results = Vec::new();
      if modes.len() == 0 {
        match read_once(&mut this, lua, ReadMode::Line).await {
          Ok(result) => results.push(result),
          Err(error) => return lua.pack_multi((Nil, error.to_string())),
        }
      } else {
        for (i, mode) in modes.into_iter().cloned().enumerate() {
          let mode = ReadMode::from_lua(mode)
            .map_err(|error| arg_error(lua, i + 2, &error.to_string(), 1))?;
          match read_once(&mut this, lua, mode).await {
            Ok(Nil) => break,
            Ok(result) => results.push(result),
            Err(error) => return lua.pack_multi((Nil, error.to_string())),
          }
        }
      }
      Ok(MultiValue::from_vec(results))
    });

    methods.add_async_function("write", |lua, args: MultiValue| async move {
      let this_u: AnyUserData = check_arg(lua, &args, 1, "file", 1)?;
      {
        let mut this = this_u
          .borrow_mut::<Self>()
          .map_err(|_| tag_error(lua, 1, "file", "other userdata", 1))?;
        for (i, x) in args.iter().cloned().enumerate().skip(1) {
          let type_name = x.type_name();
          let x = lua
            .coerce_string(x)
            .ok()
            .flatten()
            .ok_or_else(|| tag_error(lua, i, "string", type_name, 1))?;
          if let Err(error) = this.0.write_all(x.as_bytes()).await {
            return lua.pack_multi((Nil, error.to_string()));
          }
        }
      }
      lua.pack_multi(this_u)
    });

    methods.add_async_function("seek", |lua, args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "file", 1)?;
      let whence: Option<mlua::String> = check_arg(lua, &args, 2, "string", 1)?;
      let offset = check_arg::<Option<i64>>(lua, &args, 3, "integer", 1)?.unwrap_or(0);

      let seekfrom = if let Some(whence) = whence {
        match whence.as_bytes() {
          b"set" => SeekFrom::Start(
            offset
              .try_into()
              .map_err(|_| arg_error(lua, 2, "cannot combine 'set' with negative number", 1))?,
          ),
          b"cur" => SeekFrom::Current(offset),
          b"end" => SeekFrom::End(offset),
          x => {
            let msg = format!("invalid option {:?}", x.as_bstr());
            return Err(arg_error(lua, 2, &msg, 1));
          }
        }
      } else {
        SeekFrom::Current(0)
      };
      lua.pack_multi(this.0.seek(seekfrom).await.to_lua_err())
    });

    methods.add_function("lines", |lua, args: MultiValue| {
      let this = check_arg::<AnyUserData>(lua, &args, 1, "file", 0)?;
      if !this.is::<Self>() {
        return Err(tag_error(lua, 1, "file", "other userdata", 0));
      }
      // TODO: use `read_once`
      let iter = lua.create_async_function(|lua, this: AnyUserData| async move {
        let mut this = this.borrow_mut::<Self>()?;
        let result = async {
          let mut buf = Vec::new();
          if this.0.read_until(b'\n', &mut buf).await? == 0 {
            mlua::Result::Ok(Nil)
          } else {
            Ok(mlua::Value::String(lua.create_string(&buf)?))
          }
        };
        Ok(result.await)
      })?;
      iter.bind(this)
    });

    methods.add_async_function("flush", |lua, args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "file", 1)?;
      lua.pack_multi(this.0.flush().await.to_lua_err().map(|_| Nil))
    });

    methods.add_async_function("into_stream", |lua, args: MultiValue| async move {
      let this = check_arg::<AnyUserData>(lua, &args, 1, "file", 0)?
        .take::<Self>()
        .map_err(|_| tag_error(lua, 1, "file", "other userdata", 0))?;
      Ok(ByteStream::from_async_read(this.0))
    });
  }
}

fn bad_fd() -> io::Error {
  io::Error::from_raw_os_error(libc::EBADF)
}

#[pin_project(project = GenericFileProj)]
pub enum GenericFile {
  File(#[pin] File),
  ReadOnly(#[pin] ReadOnlyFile),
}

impl GenericFile {
  pub async fn len(&mut self) -> io::Result<u64> {
    match self {
      Self::File(f) => Ok(f.metadata().await?.len()),
      _ => {
        let len = self.seek(SeekFrom::End(0)).await?;
        self.rewind().await?;
        Ok(len)
      }
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
      GenericFileProj::ReadOnly(f) => f.poll_read(cx, buf),
    }
  }
}

impl AsyncWrite for GenericFile {
  fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_write(cx, buf),
      GenericFileProj::ReadOnly(_) => Poll::Ready(Err(bad_fd())),
    }
  }

  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_flush(cx),
      GenericFileProj::ReadOnly(_) => Poll::Ready(Err(bad_fd())),
    }
  }

  fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_shutdown(cx),
      GenericFileProj::ReadOnly(_) => Poll::Ready(Err(bad_fd())),
    }
  }
}

impl AsyncSeek for GenericFile {
  fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
    match self.project() {
      GenericFileProj::File(f) => f.start_seek(position),
      GenericFileProj::ReadOnly(f) => f.start_seek(position),
    }
  }

  fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
    match self.project() {
      GenericFileProj::File(f) => f.poll_complete(cx),
      GenericFileProj::ReadOnly(f) => f.poll_complete(cx),
    }
  }
}

fn create_fn_fs_open(
  lua: &Lua,
  source: Source,
  local_storage_path: Arc<Path>,
) -> mlua::Result<Function<'_>> {
  lua.create_async_function(move |lua, args: MultiValue| {
    let source = source.clone();
    let local_storage_path = local_storage_path.clone();
    async move {
      let path: mlua::String = check_arg(lua, &args, 1, "string", 1)?;
      let mode: Option<mlua::String> = check_arg(lua, &args, 2, "string", 1)?;

      let (scheme, path) = parse_path(&path)?;
      let mode = OpenMode::from_lua(mode)?;

      let file = match scheme {
        "local" => {
          let path = normalize_path_str(path);
          let file = mode
            .to_open_options()
            .open(local_storage_path.join(path))
            .await;
          match file {
            Ok(file) => GenericFile::File(file),
            Err(error) => return lua.pack_multi((Nil, error.to_string())),
          }
        }
        "source" => {
          // For `source:`, the only open mode is "read"
          match source.get(path).await {
            Ok(file) => GenericFile::ReadOnly(file),
            Err(error) => return lua.pack_multi((Nil, error.to_string())),
          }
        }
        _ => return Err(scheme_not_supported(scheme)),
      };
      let file = LuaFile(BufReader::new(file));
      let file = lua.create_userdata(file)?;
      context::register(lua, file.clone())?;
      lua.pack_multi(file)
    }
  })
}

fn create_fn_fs_mkdir(lua: &Lua, local_storage_path: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, args: MultiValue| {
    let local_storage_path = local_storage_path.clone();
    async move {
      let path: mlua::String = check_arg(lua, &args, 1, "string", 1)?;
      let all = check_arg::<Option<bool>>(lua, &args, 2, "bool", 1)?.unwrap_or(false);
      let (scheme, path) = parse_path(&path)?;

      let path: Cow<Path> = match scheme {
        "local" => local_storage_path.join(normalize_path_str(path)).into(),
        "source" => return Err(rt_error("cannot modify service source")),
        _ => return Err(scheme_not_supported(scheme)),
      };

      let result = async move {
        if all {
          fs::create_dir_all(path).await?;
        } else {
          fs::create_dir(path).await?;
        }
        mlua::Result::Ok(Nil)
      };
      Ok(result.await)
    }
  })
}

fn create_fn_fs_remove(lua: &Lua, local_storage_path: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, args: MultiValue| {
    let local_storage_path = local_storage_path.clone();
    async move {
      let path: mlua::String = check_arg(lua, &args, 1, "string", 1)?;
      let all = check_arg::<Option<bool>>(lua, &args, 2, "bool", 1)?.unwrap_or(false);

      let (scheme, path) = parse_path(&path)?;
      let path: Cow<Path> = match scheme {
        "local" => local_storage_path.join(normalize_path_str(path)).into(),
        "source" => return Err(rt_error("cannot modify service source")),
        _ => return Err(scheme_not_supported(scheme)),
      };

      let result = async move {
        let metadata = fs::metadata(&path).await?;
        if metadata.is_dir() {
          if all {
            fs::remove_dir_all(path).await?;
          } else {
            fs::remove_dir(path).await?;
          }
        } else {
          fs::remove_file(path).await?;
        }
        mlua::Result::Ok(Nil)
      };
      Ok(result.await)
    }
  })
}

fn parse_path<'a>(path: &'a mlua::String<'a>) -> mlua::Result<(&'a str, &'a str)> {
  let path = path.as_bytes();
  let path =
    std::str::from_utf8(path).map_err(|_| rt_error_fmt!("invalid path: {:?}", path.as_bstr()))?;
  Ok(path.split_once(':').unwrap_or(("local", path)))
}

fn scheme_not_supported(scheme: &str) -> mlua::Error {
  rt_error_fmt!("scheme currently not supported: {scheme}")
}
