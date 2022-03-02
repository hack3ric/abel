use crate::lua::byte_stream::ByteStream;
use crate::lua::BadArgument;
use crate::Source;
use mlua::{
  AnyUserData, ExternalError, ExternalResult, Function, Lua, String as LuaString, UserData,
  UserDataMethods, Variadic,
};
use std::io::SeekFrom;
use tokio::fs::{File, OpenOptions};
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
  fn from_lua(mode: Option<LuaString>) -> mlua::Result<Self> {
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
  fn from_lua(mode: Option<mlua::Value>) -> mlua::Result<Self> {
    if let Some(mode) = mode {
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
    } else {
      Ok(Self::Line)
    }
  }
}

pub struct LuaFile(BufReader<File>);

impl UserData for LuaFile {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_function("__close", |_lua, this: AnyUserData| {
      drop(this.take::<Self>());
      Ok(())
    });

    // TODO: multiple modes, according to Lua
    methods.add_async_function(
      "read",
      |lua, (this, mode): (AnyUserData, Option<mlua::Value>)| async move {
        use ReadMode::*;
        let mut this = this.borrow_mut::<Self>()?;
        let mode = ReadMode::from_lua(mode)
          .map_err(|error| BadArgument::new("read", 1, error.to_string()))?;
        match mode {
          All => {
            let file_ref = this.0.get_mut();
            let file_len = file_ref.metadata().await?.len();
            let pos = file_ref.seek(SeekFrom::Current(0)).await?;
            let len = file_len - pos;
            let mut buf = Vec::with_capacity(len as _);
            this.0.read_to_end(&mut buf).await?;
            Ok(lua.create_string(&buf))
          }
          Exact(len) => {
            let len = len.min(this.0.get_ref().metadata().await?.len() + 1);
            let mut buf = vec![0; len as _];
            let actual_len = this.0.read_exact(&mut buf).await?;
            buf.truncate(actual_len);
            Ok(lua.create_string(&buf))
          }
          Line => {
            let mut buf = String::new();
            let bytes = this.0.read_line(&mut buf).await?;
            if bytes == 0 {
              Err("eof reached".to_lua_err())
            } else {
              if buf.ends_with('\n') {
                buf.pop();
              }
              if buf.ends_with('\r') {
                buf.pop();
              }
              Ok(lua.create_string(&buf))
            }
          }
          LineWithDelimiter => {
            let mut buf = String::new();
            let bytes = this.0.read_line(&mut buf).await?;
            if bytes == 0 {
              Err("eof reached".to_lua_err())
            } else {
              Ok(lua.create_string(&buf))
            }
          }
        }
      },
    );

    methods.add_async_function(
      "write",
      |_lua, (this, content): (AnyUserData, Variadic<LuaString>)| async move {
        let mut this = this.borrow_mut::<Self>()?;
        for x in content {
          this.0.write_all(x.as_bytes()).await?;
        }
        Ok(())
      },
    );

    methods.add_async_function("flush", |_lua, this: AnyUserData| async move {
      let mut this = this.borrow_mut::<Self>()?;
      this.0.flush().await?;
      Ok(())
    });

    methods.add_async_function(
      "seek",
      |_lua, (this, whence, offset): (AnyUserData, Option<LuaString>, Option<i64>)| async move {
        let mut this = this.borrow_mut::<Self>()?;
        let offset = offset.unwrap_or(0);
        let seekfrom = if let Some(whence) = whence {
          match whence.as_bytes() {
            b"set" => SeekFrom::Start(offset.try_into().to_lua_err()?),
            b"cur" => SeekFrom::Current(offset),
            b"end" => SeekFrom::End(offset),
            _ => return Err("invalid seek base".to_lua_err()),
          }
        } else {
          SeekFrom::Current(0)
        };
        Ok(this.0.seek(seekfrom).await?)
      },
    );

    methods.add_async_function("into_stream", |_lua, this: AnyUserData| async move {
      let this = this.take::<Self>()?;
      Ok(ByteStream::from_async_read(this.0))
    });
  }
}

fn create_fn_fs_open(lua: &Lua, source: Source) -> mlua::Result<Function> {
  lua.create_async_function(move |_lua, (path, mode): (LuaString, Option<LuaString>)| {
    let source = source.clone();
    async move {
      // TODO: source file open
      let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
      let (scheme, path) = path.split_once(':').unwrap_or(("global", path));
      if scheme != "source" {
        return Err(format!("scheme currently not supported: {scheme}").to_lua_err());
      }
      // For `source:`, the only open mode is "read".
      let _mode = OpenMode::from_lua(mode)?;
      let file = source.get(path).await?;
      Ok(LuaFile(BufReader::new(file)))
    }
  })
}

pub fn create_preload_fs(lua: &Lua, source: Source) -> mlua::Result<Function> {
  lua.create_function(move |lua, ()| {
    let fs_table = lua.create_table()?;
    fs_table.raw_set("open", create_fn_fs_open(lua, source.clone())?)?;
    Ok(fs_table)
  })
}
