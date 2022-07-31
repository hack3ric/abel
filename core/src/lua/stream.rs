use super::error::{check_userdata_mut, rt_error, tag_handler};
use super::LuaCacheExt;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use hyper::body::Bytes;
use hyper::Body;
use mlua::Value::Nil;
use mlua::{
  AnyUserData, Lua, MultiValue, UserData, UserDataFields,
  UserDataMethods,
};
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;
use super::json::create_fn_json_parse;

/// - Stream: `stream<T>:read() -> T?`
/// - Sink: `sink<T>:write(item: T)`
/// - Transform: `transform<T, U>:transform(item: T) -> U`
pub fn create_preload_stream(lua: &Lua) -> mlua::Result<mlua::Function> {
  lua.create_cached_function("abel:preload_stream", |lua, ()| create_table_stream(lua))
}

pub(crate) fn create_table_stream(lua: &Lua) -> mlua::Result<mlua::Table> {
  lua.create_cached_value("abel:stream_module", |lua| {
    let stream = lua
      .load(include_str!("stream.lua"))
      .set_name("@[stream]")?
      .call(create_fn_json_parse(lua)?)?;
    Ok(stream)
  })
}

pub struct ByteStream(pub(crate) BoxStream<'static, mlua::Result<Bytes>>);

impl ByteStream {
  pub fn from_async_read(r: impl AsyncRead + Send + 'static) -> Self {
    Self(ReaderStream::new(r).map_err(rt_error).boxed())
  }
}

impl From<Body> for ByteStream {
  fn from(body: Body) -> Self {
    Self(body.map_err(rt_error).boxed())
  }
}

impl UserData for ByteStream {
  fn add_fields<'lua, F: UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_meta_field_with("__index", create_table_stream);
  }

  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    // Added since a file may turn into `ByteStream`
    methods.add_meta_function("__close", |_lua, this: AnyUserData| {
      drop(this.take::<Self>());
      Ok(())
    });

    methods.add_async_function("read", |lua, mut args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(args.pop_front(), "byte stream")
        .map_err(tag_handler(lua, 1, 1))?;
      let value = match this.with_borrowed_mut(|x| x.0.try_next()).await? {
        Some(bytes) => mlua::Value::String(lua.create_string(&bytes)?),
        None => Nil,
      };
      Ok(value)
    });
  }
}
