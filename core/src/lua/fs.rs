use super::error::{arg_error, check_truthiness, check_userdata_mut, rt_error, tag_error};
use super::LuaCacheExt;
use crate::lua::byte_stream::ByteStream;
use crate::lua::context;
use crate::lua::error::{
  check_integer, check_string, check_userdata, check_value, rt_error_fmt, tag_handler_async,
  UserDataRef, UserDataRefMut,
};
use crate::path::normalize_path_str;
use crate::source::{ReadOnlyFile, Source};
use bstr::ByteSlice;
use mlua::Value::Nil;
use mlua::{AnyUserData, ExternalResult, Function, Lua, MultiValue, UserData, UserDataMethods};
use pin_project::pin_project;
use std::borrow::Cow;
use std::io::SeekFrom;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tempfile::tempfile;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{
  self, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite,
  AsyncWriteExt, BufReader,
};
use tokio::task::spawn_blocking;

pub fn create_preload_fs(
  source: Source,
  local_storage_path: Arc<Path>,
) -> impl FnOnce(&Lua) -> mlua::Result<Function> {
  |lua| {
    lua.create_function(move |lua, ()| {
      let fs = lua.create_table()?;
      fs.raw_set(
        "open",
        create_fn_fs_open(lua, source.clone(), local_storage_path.clone())?,
      )?;
      fs.raw_set("type", create_fn_fs_type(lua)?)?;
      fs.raw_set("tmpfile", create_fn_fs_tmpfile(lua)?)?;
      fs.raw_set(
        "mkdir",
        create_fn_fs_mkdir(lua, local_storage_path.clone())?,
      )?;
      fs.raw_set(
        "remove",
        create_fn_fs_remove(lua, local_storage_path.clone())?,
      )?;
      Ok(fs)
    })
  }
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
  fn from_lua(mode: mlua::Value) -> Result<Self, String> {
    match mode {
      mlua::Value::Integer(i) if i > 0 => Ok(Self::Exact(i as _)),
      mlua::Value::Integer(_) => Err("read bytes cannot be negative".into()),
      mlua::Value::String(s) => match s.as_bytes() {
        b"a" => Ok(Self::All),
        b"l" => Ok(Self::Line),
        b"L" => Ok(Self::LineWithDelimiter),
        s => Err(format!("invalid file read mode {:?}", s.as_bstr())),
      },
      _ => Err(format!(
        "string or integer expected, got {}",
        mode.type_name(),
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
    fn check_self_async<'lua>(
      lua: &'lua Lua,
      value: Option<mlua::Value<'lua>>,
    ) -> mlua::Result<UserDataRef<'lua, LuaFile>> {
      check_userdata(value, "file").map_err(tag_handler_async(lua, 1))
    }

    fn check_self_mut_async<'lua>(
      lua: &'lua Lua,
      value: Option<mlua::Value<'lua>>,
    ) -> mlua::Result<UserDataRefMut<'lua, LuaFile>> {
      check_userdata_mut(value, "file").map_err(tag_handler_async(lua, 1))
    }

    methods.add_meta_function("__close", |_lua, this: AnyUserData| {
      drop(this.take::<Self>());
      Ok(())
    });

    methods.add_async_function("read", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      let modes = args.into_iter();
      let mut results = Vec::new();
      if modes.len() == 0 {
        let result = this
          .with_borrowed_mut(|x| read_once(x, lua, ReadMode::Line))
          .await;
        match result {
          Ok(result) => results.push(result),
          Err(error) => return lua.pack_multi((Nil, error.to_string())),
        }
      } else {
        for (i, mode) in modes.enumerate() {
          let mode = ReadMode::from_lua(mode).map_err(|error| arg_error(lua, i + 2, &error, 1))?;
          match this.with_borrowed_mut(|x| read_once(x, lua, mode)).await {
            Ok(Nil) => break,
            Ok(result) => results.push(result),
            Err(error) => return lua.pack_multi((Nil, error.to_string())),
          }
        }
      }
      Ok(MultiValue::from_vec(results))
    });

    methods.add_async_function("write", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      for (i, x) in args.iter().cloned().enumerate().skip(1) {
        let type_name = x.type_name();
        let x = lua
          .coerce_string(x)
          .ok()
          .flatten()
          .ok_or_else(|| tag_error(lua, i, "string", type_name, 1))?;
        if let Err(error) = this
          .with_borrowed_mut(|t| t.0.write_all(x.as_bytes()))
          .await
        {
          return lua.pack_multi((Nil, error.to_string()));
        }
      }
      lua.pack_multi(this.into_any())
    });

    methods.add_async_function("seek", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      let whence: Option<mlua::String> = args
        .pop_front()
        .map(|x| check_string(lua, Some(x)))
        .transpose()
        .map_err(tag_handler_async(lua, 2))?;
      let offset = args
        .pop_front()
        .map(|x| check_integer(Some(x)))
        .unwrap_or(Ok(0))
        .map_err(tag_handler_async(lua, 3))?;

      let seekfrom = if let Some(whence) = whence {
        match whence.as_bytes() {
          b"set" => {
            let offset = offset
              .try_into()
              .map_err(|_| arg_error(lua, 2, "cannot combine 'set' with negative number", 1))?;
            SeekFrom::Start(offset)
          }
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
      lua.pack_multi(
        this
          .with_borrowed_mut(|x| x.0.seek(seekfrom))
          .await
          .to_lua_err(),
      )
    });

    methods.add_function("lines", |lua, mut args: MultiValue| {
      let this = check_self_async(lua, args.pop_front())?;
      let mode = args
        .pop_front()
        .map(ReadMode::from_lua)
        .unwrap_or(Ok(ReadMode::Line))
        .map_err(|error| arg_error(lua, 2, &error, 1))?;
      let iter = lua.create_async_function(move |lua, this: AnyUserData| async move {
        let mut this = this.borrow_mut::<Self>()?;
        // This, unlike other function in `fs`, returns hard error.
        // This corresponds with Lua's behaviour.
        read_once(&mut this, lua, mode).await
      })?;
      iter.bind(this.into_any())
    });

    methods.add_async_function("flush", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      lua.pack_multi(
        this
          .with_borrowed_mut(|x| x.0.flush())
          .await
          .to_lua_err()
          .map(|_| Nil),
      )
    });

    methods.add_async_function("into_stream", |lua, mut args: MultiValue| async move {
      let this = check_value::<AnyUserData>(lua, args.pop_front(), "file")
        .map_err(tag_handler_async(lua, 1))?
        .take::<Self>()
        .map_err(|_| tag_error(lua, 1, "file", "other userdata", 1))?;
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

// Also used in `io.open`
pub(crate) fn create_fn_fs_open(
  lua: &Lua,
  source: Source,
  local_storage_path: Arc<Path>,
) -> mlua::Result<Function<'_>> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let source = source.clone();
    let local_storage_path = local_storage_path.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler_async(lua, 1))?;
      let mode = args
        .pop_front()
        .map(|x| check_string(lua, Some(x)))
        .transpose()
        .map_err(tag_handler_async(lua, 2))?;

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

// Also used in `io.type`
pub(crate) fn create_fn_fs_type(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:fs.type", |lua, mut args: MultiValue| {
    use mlua::Value::*;
    let maybe_file = args
      .pop_front()
      .ok_or_else(|| arg_error(lua, 1, "value expected", 0))?;
    match maybe_file {
      UserData(u) if u.is::<LuaFile>() => Ok(String(lua.create_string("file")?)),
      UserData(_) => Ok(String(lua.create_string("closed file")?)),
      _ => Ok(Nil),
    }
  })
}

// Also used in `io.tmpfile`
pub(crate) fn create_fn_fs_tmpfile(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_async_function("abel:fs.tmpfile", |_lua, ()| async move {
    let result = spawn_blocking(tempfile)
      .await
      .map_err(|_| io::Error::new(io::ErrorKind::Other, "background task failed"))?
      .map(|file| LuaFile(BufReader::new(GenericFile::File(File::from_std(file)))))
      .to_lua_err();
    Ok(result)
  })
}

fn create_fn_fs_mkdir(lua: &Lua, local_storage_path: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let local_storage_path = local_storage_path.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler_async(lua, 1))?;
      let all = check_truthiness(args.pop_front());

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
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let local_storage_path = local_storage_path.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler_async(lua, 1))?;
      let all = check_truthiness(args.pop_front());

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

// Simplified version of `fs.remove`
pub(crate) fn create_fn_os_remove(
  lua: &Lua,
  local_storage_path: Arc<Path>,
) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let local_storage_path = local_storage_path.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler_async(lua, 1))?;

      let (scheme, path) = parse_path(&path)?;
      let path: Cow<Path> = match scheme {
        "local" => local_storage_path.join(normalize_path_str(path)).into(),
        "source" => return Err(rt_error("cannot modify service source")),
        _ => return Err(scheme_not_supported(scheme)),
      };

      let result = async move {
        let metadata = fs::metadata(&path).await?;
        if metadata.is_dir() {
          fs::remove_dir(path).await?
        } else {
          fs::remove_file(path).await?
        }
        mlua::Result::Ok(Nil)
      };
      Ok(result.await)
    }
  })
}

// TODO: create_fn_fs_rename

fn parse_path<'a>(path: &'a mlua::String<'a>) -> mlua::Result<(&'a str, &'a str)> {
  let path = path.as_bytes();
  let path =
    std::str::from_utf8(path).map_err(|_| rt_error_fmt!("invalid path: {:?}", path.as_bstr()))?;
  Ok(path.split_once(':').unwrap_or(("local", path)))
}

fn scheme_not_supported(scheme: &str) -> mlua::Error {
  rt_error_fmt!("scheme currently not supported: {scheme}")
}
