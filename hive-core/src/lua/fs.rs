use crate::lua::byte_stream::ByteStream;
use crate::lua::BadArgument;
use crate::path::{normalize_path, normalize_path_str};
use crate::permission::{Permission, PermissionSet};
use crate::source::{DirSource, GenericFile, Source};
use crate::{HiveState, Result};
use mlua::{
  AnyUserData, ExternalError, ExternalResult, Function, Lua, MultiValue, ToLua, UserData,
  UserDataMethods, Variadic,
};
use std::borrow::Cow;
use std::io::SeekFrom;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};

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
        _ => return Err("invalid open mode".to_lua_err()),
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
      mlua::Value::Integer(i) => {
        if i > 0 {
          return Ok(Self::Exact(i as _));
        }
      }
      mlua::Value::String(s) => match s.as_bytes() {
        b"a" => return Ok(Self::All),
        b"l" => return Ok(Self::Line),
        b"L" => return Ok(Self::LineWithDelimiter),
        _ => (),
      },
      _ => (),
    }
    Err("invalid file read mode".to_lua_err())
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
      let file_len = file_ref.metadata().await?.len();
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
        let len = len.min(this.0.get_ref().metadata().await?.len());
        let mut buf = vec![0; len as _];
        let actual_len = this.0.read_exact(&mut buf).await?;
        if actual_len == 0 {
          Ok(mlua::Value::Nil)
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
        Ok(mlua::Value::Nil)
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
        Ok(mlua::Value::Nil)
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

    methods.add_async_function(
      "read",
      |lua, (this, modes): (AnyUserData, MultiValue)| async move {
        let mut this = this.borrow_mut::<Self>()?;
        let mut results = Vec::new();
        if modes.is_empty() {
          results.push(read_once(&mut this, lua, ReadMode::Line).await?);
        } else {
          for (i, mode) in modes.into_iter().enumerate() {
            let mode = ReadMode::from_lua(mode)
              .map_err(|error| BadArgument::new("read", i as u8 + 1, error.to_string()))?;
            let result = read_once(&mut this, lua, mode).await?;
            if let mlua::Value::Nil = result {
              results.push(result);
              break;
            } else {
              results.push(result);
            }
          }
        }
        Ok(MultiValue::from_vec(results))
      },
    );

    methods.add_async_function(
      "write",
      |_lua, (this, content): (AnyUserData, Variadic<mlua::String>)| async move {
        let mut this = this.borrow_mut::<Self>()?;
        for x in content {
          this.0.write_all(x.as_bytes()).await?;
        }
        Ok(())
      },
    );

    methods.add_async_function(
      "seek",
      |_lua, (this, whence, offset): (AnyUserData, Option<mlua::String>, Option<i64>)| async move {
        let mut this = this.borrow_mut::<Self>()?;
        let offset = offset.unwrap_or(0);
        let seekfrom = if let Some(whence) = whence {
          match whence.as_bytes() {
            b"set" => SeekFrom::Start(offset.try_into().to_lua_err()?),
            b"cur" => SeekFrom::Current(offset),
            b"end" => SeekFrom::End(offset),
            x => {
              return Err(format!("invalid seek base: {}", String::from_utf8_lossy(x)).to_lua_err())
            }
          }
        } else {
          SeekFrom::Current(0)
        };
        Ok(this.0.seek(seekfrom).await?)
      },
    );

    methods.add_function("lines", |lua, this: AnyUserData| {
      let iter = lua.create_async_function(|lua, this: AnyUserData| async move {
        let mut this = this.borrow_mut::<Self>()?;
        let mut buf = Vec::new();
        this.0.read_until(b'\n', &mut buf).await?;
        lua.create_string(&buf)
      })?;
      iter.bind(this)
    });

    methods.add_async_function("flush", |_lua, this: AnyUserData| async move {
      let mut this = this.borrow_mut::<Self>()?;
      this.0.flush().await?;
      Ok(())
    });

    methods.add_async_function("into_stream", |_lua, this: AnyUserData| async move {
      let this = this.take::<Self>()?;
      Ok(ByteStream::from_async_read(this.0))
    });
  }
}

fn create_fn_fs_open(
  lua: &Lua,
  source: DirSource,
  local_storage_path: Arc<Path>,
  permissions: Arc<PermissionSet>,
) -> mlua::Result<Function<'_>> {
  lua.create_async_function(
    move |_lua, (path, mode): (mlua::String, Option<mlua::String>)| {
      use OpenMode::*;
      let source = source.clone();
      let local_storage_path = local_storage_path.clone();
      let permissions = permissions.clone();
      async move {
        let (scheme, path) = parse_path(&path)?;
        let mode = OpenMode::from_lua(mode)?;
        let file = match scheme {
          "local" => {
            let path = normalize_path_str(path);
            Box::pin(
              mode
                .to_open_options()
                .open(local_storage_path.join(path))
                .await?,
            )
          }
          "external" => {
            let path = normalize_path(path);
            let read = Permission::Read {
              path: Cow::Borrowed(&path),
            };
            let write = Permission::Write {
              path: Cow::Borrowed(&path),
            };
            match mode {
              Read => permissions.check(&read)?,
              Write | Append => permissions.check(&write)?,
              ReadWrite | ReadWriteNew | ReadAppend => {
                permissions.check(&read)?;
                permissions.check(&write)?;
              }
            }
            Box::pin(mode.to_open_options().open(path).await?)
          }
          "source" => {
            // For `source:`, the only open mode is "read"
            source.get(path).await?
          }
          _ => return scheme_not_supported(scheme),
        };
        Ok(LuaFile(BufReader::new(file)))
      }
    },
  )
}

pub async fn create_preload_fs<'lua>(
  lua: &'lua Lua,
  state: &HiveState,
  service_name: &str,
  source: DirSource,
  permissions: Arc<PermissionSet>,
) -> mlua::Result<Function<'lua>> {
  let local_storage_path: Arc<Path> = state.local_storage_path.join(service_name).into();
  if !local_storage_path.exists() {
    tokio::fs::create_dir(&local_storage_path).await?;
  }

  lua.create_function(move |lua, ()| {
    let fs_table = lua.create_table()?;
    fs_table.raw_set(
      "open",
      create_fn_fs_open(
        lua,
        source.clone(),
        local_storage_path.clone(),
        permissions.clone(),
      )?,
    )?;
    fs_table.raw_set(
      "mkdir",
      create_fn_fs_mkdir(lua, local_storage_path.clone(), permissions.clone())?,
    )?;
    fs_table.raw_set(
      "remove",
      create_fn_fs_remove(lua, local_storage_path.clone(), permissions.clone())?,
    )?;
    Ok(fs_table)
  })
}

fn create_fn_fs_mkdir(
  lua: &Lua,
  local_storage_path: Arc<Path>,
  permissions: Arc<PermissionSet>,
) -> mlua::Result<Function> {
  lua.create_async_function(move |_lua, (path, all): (mlua::String, bool)| {
    let local_storage_path = local_storage_path.clone();
    let permissions = permissions.clone();
    async move {
      let (scheme, path) = parse_path(&path)?;

      let path: Cow<Path> = match scheme {
        "local" => local_storage_path.join(normalize_path_str(path)).into(),
        "external" => {
          permissions.check(&Permission::Write {
            path: Cow::Borrowed(Path::new(path)),
          })?;
          Path::new(path).into()
        }
        "source" => return Err("cannot modify service source".to_lua_err()),
        _ => return scheme_not_supported(scheme),
      };

      if all {
        fs::create_dir_all(path).await?;
      } else {
        fs::create_dir(path).await?;
      }
      Ok(())
    }
  })
}

fn create_fn_fs_remove(
  lua: &Lua,
  local_storage_path: Arc<Path>,
  permissions: Arc<PermissionSet>,
) -> mlua::Result<Function> {
  lua.create_async_function(move |_lua, (path, all): (mlua::String, bool)| {
    let local_storage_path = local_storage_path.clone();
    let permissions = permissions.clone();
    async move {
      let (scheme, path) = parse_path(&path)?;

      let path: Cow<Path> = match scheme {
        "local" => local_storage_path.join(normalize_path_str(path)).into(),
        "external" => {
          let path: Cow<_> = Path::new(path).into();
          permissions.check(&Permission::Write { path: path.clone() })?;
          path
        }
        "source" => return Err("cannot modify service source".to_lua_err()),
        _ => return scheme_not_supported(scheme),
      };

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
      Ok(())
    }
  })
}

fn parse_path<'a>(path: &'a mlua::String<'a>) -> mlua::Result<(&'a str, &'a str)> {
  let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
  Ok(path.split_once(':').unwrap_or(("local", path)))
}

fn scheme_not_supported<T>(scheme: &str) -> mlua::Result<T> {
  Err(format!("scheme currently not supported: {scheme}").to_lua_err())
}

pub async fn remove_service_local_storage(state: &HiveState, service_name: &str) -> Result<()> {
  let path = state.local_storage_path.join(service_name);
  Ok(tokio::fs::remove_dir_all(path).await?)
}
