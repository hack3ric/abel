use super::stream::create_table_stream;
use crate::lua::error::{
  arg_error, check_integer, check_string, check_truthiness, check_userdata, check_userdata_mut,
  rt_error, rt_error_fmt, tag_error, tag_handler, UserDataRef, UserDataRefMut,
};
use crate::lua::LuaCacheExt;
use crate::path::normalize_path_str;
use crate::source::{Metadata, ReadOnlyFile, Source};
use crate::task::TaskContext;
use bstr::ByteSlice;
use mlua::Value::Nil;
use mlua::{AnyUserData, Function, Lua, MultiValue, UserData, UserDataMethods};
use pin_project::pin_project;
use std::io::SeekFrom;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tempfile::tempfile;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{
  self, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite,
  AsyncWriteExt, BufStream,
};
use tokio::task::spawn_blocking;

// Note that "lsp" stands for "local storage path".
pub fn create_preload_fs(
  source: Source,
  lsp: Arc<Path>,
) -> impl FnOnce(&Lua) -> mlua::Result<Function> {
  |lua| {
    lua.create_function(move |lua, ()| {
      let fs = lua.create_table()?;
      fs.raw_set("open", create_fn_fs_open(lua, source.clone(), lsp.clone())?)?;
      fs.raw_set("type", create_fn_fs_type(lua)?)?;
      fs.raw_set("tmpfile", create_fn_fs_tmpfile(lua)?)?;
      fs.raw_set("mkdir", create_fn_fs_mkdir(lua, lsp.clone())?)?;
      fs.raw_set("remove", create_fn_fs_remove(lua, lsp.clone())?)?;
      fs.raw_set("rename", create_fn_fs_rename(lua, lsp.clone())?)?;
      fs.raw_set(
        "metadata",
        create_fn_fs_metadata(lua, source.clone(), lsp.clone())?,
      )?;
      fs.raw_set(
        "exists",
        create_fn_fs_exists(lua, source.clone(), lsp.clone())?,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
  Local,
  Source,
}

impl Scheme {
  fn from_str(s: &str) -> mlua::Result<Self> {
    match s {
      "local" => Ok(Self::Local),
      "source" => Ok(Self::Source),
      _ => Err(rt_error_fmt!("scheme currently not supported: {s}")),
    }
  }
}

fn parse_path<'a>(path: &'a mlua::String<'a>) -> mlua::Result<(Scheme, &'a str)> {
  let path = path.as_bytes();
  let path =
    std::str::from_utf8(path).map_err(|_| rt_error_fmt!("invalid path: '{}'", path.as_bstr()))?;
  path
    .split_once(':')
    .map(|(s, p)| mlua::Result::Ok((Scheme::from_str(s)?, p)))
    .unwrap_or(Ok((Scheme::Local, path)))
}

pub struct LuaFile(pub(crate) BufStream<GenericFile>);

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
  fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {
    _fields.add_meta_field_with("__index", create_table_stream);
  }

  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    fn check_self_async<'lua>(
      lua: &'lua Lua,
      value: Option<mlua::Value<'lua>>,
    ) -> mlua::Result<UserDataRef<'lua, LuaFile>> {
      check_userdata(value, "file").map_err(tag_handler(lua, 1, 1))
    }

    fn check_self_mut_async<'lua>(
      lua: &'lua Lua,
      value: Option<mlua::Value<'lua>>,
    ) -> mlua::Result<UserDataRefMut<'lua, LuaFile>> {
      check_userdata_mut(value, "file").map_err(tag_handler(lua, 1, 1))
    }

    async fn close(_lua: &Lua, this: AnyUserData<'_>) -> mlua::Result<()> {
      if let Ok(mut this) = this.take::<LuaFile>() {
        this.0.flush().await.map_err(rt_error)?;
      }
      Ok(())
    }

    methods.add_async_meta_function("__close", close);
    methods.add_async_function("close", close);

    methods.add_async_function("read", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      let modes = args.into_iter();

      if modes.len() == 0 {
        let reader = this.with_borrowed_mut(|x| &mut x.0);
        let buf = reader.fill_buf().await.map_err(rt_error)?;
        let len = buf.len();
        let result = if len == 0 {
          Nil
        } else {
          mlua::Value::String(lua.create_string(buf)?)
        };
        reader.consume(len);
        lua.pack_multi(result)
      } else {
        let mut results = Vec::new();
        for (i, mode) in modes.enumerate() {
          let mode = ReadMode::from_lua(mode).map_err(|error| arg_error(lua, i + 2, &error, 1))?;
          match this.with_borrowed_mut(|x| read_once(x, lua, mode)).await? {
            Nil => break,
            result => results.push(result),
          }
        }
        Ok(MultiValue::from_vec(results))
      }
    });

    methods.add_async_function("write", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      for (i, x) in args.iter().cloned().enumerate() {
        let type_name = x.type_name();
        let x = lua
          .coerce_string(x)
          .ok()
          .flatten()
          .ok_or_else(|| tag_error(lua, i + 1, "string", type_name, 1))?;
        this
          .with_borrowed_mut(|t| t.0.write_all(x.as_bytes()))
          .await
          .map_err(rt_error)?;
      }
      Ok(this.into_any())
    });

    methods.add_async_function("seek", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      let whence: Option<mlua::String> = args
        .pop_front()
        .map(|x| check_string(lua, Some(x)))
        .transpose()
        .map_err(tag_handler(lua, 2, 1))?;
      let offset = args
        .pop_front()
        .map(|x| check_integer(Some(x)))
        .unwrap_or(Ok(0))
        .map_err(tag_handler(lua, 3, 1))?;

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

      let inner = this.with_borrowed_mut(|x| &mut x.0);
      inner.flush().await.map_err(rt_error)?;
      inner.seek(seekfrom).await.map_err(rt_error)
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
        read_once(&mut this, lua, mode).await
      })?;
      iter.bind(this.into_any())
    });

    methods.add_async_function("flush", |lua, mut args: MultiValue| async move {
      let mut this = check_self_mut_async(lua, args.pop_front())?;
      this
        .with_borrowed_mut(|x| x.0.flush())
        .await
        .map_err(rt_error)
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

fn create_fn_fs_open(lua: &Lua, source: Source, lsp: Arc<Path>) -> mlua::Result<Function<'_>> {
  use OpenMode::*;
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let source = source.clone();
    let lsp = lsp.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
      let mode = args
        .pop_front()
        .map(|x| check_string(lua, Some(x)))
        .transpose()
        .map_err(tag_handler(lua, 2, 1))?;

      let (scheme, path) = parse_path(&path)?;
      let mode = OpenMode::from_lua(mode)?;

      let file = match scheme {
        Scheme::Local => {
          let path = normalize_path_str(path);
          mode
            .to_open_options()
            .open(lsp.join(path))
            .await
            .map(GenericFile::File)
            .map_err(rt_error)?
        }
        Scheme::Source => {
          // For `source:`, the only open mode is "read"
          source
            .get(path)
            .await
            .map(GenericFile::ReadOnly)
            .map_err(rt_error)?
        }
      };
      let (rc, wc) = match mode {
        Read => (8192, 0),
        Write | Append => (0, 8192),
        _ => (8192, 8192),
      };
      let file = lua.create_userdata(LuaFile(BufStream::with_capacity(rc, wc, file)))?;
      TaskContext::register(lua, file.clone())?;
      Ok(file)
    }
  })
}

fn create_fn_fs_type(lua: &Lua) -> mlua::Result<Function> {
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

fn create_fn_fs_tmpfile(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_async_function("abel:fs.tmpfile", |_lua, ()| async move {
    spawn_blocking(tempfile)
      .await
      .map_err(|x| rt_error_fmt!("background task failed: {x}"))?
      .map(|file| LuaFile(BufStream::new(GenericFile::File(File::from_std(file)))))
      .map_err(rt_error)
  })
}

fn create_fn_fs_mkdir(lua: &Lua, lsp: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let lsp = lsp.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
      let all = check_truthiness(args.pop_front());

      let (scheme, path) = parse_path(&path)?;
      let path = match scheme {
        Scheme::Local => lsp.join(normalize_path_str(path)),
        Scheme::Source => return Err(rt_error("cannot modify service source")),
      };

      let result = if all {
        fs::create_dir_all(path).await
      } else {
        fs::create_dir(path).await
      };
      result.map_err(rt_error)
    }
  })
}

fn create_fn_fs_remove(lua: &Lua, lsp: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let lsp = lsp.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
      let all = check_truthiness(args.pop_front());

      let (scheme, path) = parse_path(&path)?;
      let path = match scheme {
        Scheme::Local => lsp.join(normalize_path_str(path)),
        Scheme::Source => return Err(rt_error("cannot modify service source")),
      };

      let result = {
        let metadata = fs::metadata(&path).await?;
        if metadata.is_dir() {
          if all {
            fs::remove_dir_all(path).await
          } else {
            fs::remove_dir(path).await
          }
        } else {
          fs::remove_file(path).await
        }
      };
      result.map_err(rt_error)
    }
  })
}

fn create_fn_fs_rename(lua: &Lua, lsp: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let lsp = lsp.clone();
    async move {
      let from = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
      let to = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 2, 1))?;
      let (from_scheme, from) = parse_path(&from)?;
      let (to_scheme, to) = parse_path(&to)?;

      if from_scheme == Scheme::Local && to_scheme == Scheme::Local {
        let from = lsp.join(normalize_path_str(from));
        let to = lsp.join(normalize_path_str(to));
        fs::rename(from, to).await.map_err(rt_error)
      } else {
        Err(rt_error("'rename' only works on local storage"))
      }
    }
  })
}

fn create_fn_fs_metadata(lua: &Lua, source: Source, lsp: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let source = source.clone();
    let lsp = lsp.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
      let (scheme, path) = parse_path(&path)?;

      let result = match scheme {
        Scheme::Local => {
          let path = lsp.join(normalize_path_str(path));
          async {
            let md = fs::metadata(path).await?;
            if md.is_dir() {
              Ok(Metadata::Dir)
            } else if md.is_file() {
              Ok(Metadata::File { size: md.len() })
            } else {
              Err(rt_error("the entity is neither a file nor a directory"))
            }
          }
          .await
        }
        Scheme::Source => Ok(source.metadata(&normalize_path_str(path)).await?),
      };
      match result {
        Ok(md) => {
          let t = lua.create_table()?;
          match md {
            Metadata::Dir => t.raw_set("kind", "dir")?,
            Metadata::File { size } => {
              t.raw_set("kind", "file")?;
              t.raw_set("size", size)?;
            }
          }
          Ok(t)
        }
        Err(e) => Err(e),
      }
    }
  })
}

fn create_fn_fs_exists(lua: &Lua, source: Source, lsp: Arc<Path>) -> mlua::Result<Function> {
  lua.create_async_function(move |lua, mut args: MultiValue| {
    let source = source.clone();
    let lsp = lsp.clone();
    async move {
      let path = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
      let (scheme, path) = parse_path(&path)?;

      match scheme {
        Scheme::Local => Ok(lsp.join(normalize_path_str(path)).exists()),
        Scheme::Source => source.exists(path).await.map_err(rt_error),
      }
    }
  })
}
