use super::error::{check_userdata_mut, rt_error, tag_handler};
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use hyper::body::Bytes;
use hyper::Body;
use mlua::Value::Nil;
use mlua::{AnyUserData, ExternalResult, LuaSerdeExt, MultiValue, UserData, UserDataMethods};
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;

pub struct ByteStream(pub(crate) BoxStream<'static, mlua::Result<Bytes>>);

impl ByteStream {
  pub fn from_async_read(r: impl AsyncRead + Send + 'static) -> Self {
    Self(ReaderStream::new(r).map_err(rt_error).boxed())
  }

  async fn aggregate(&mut self) -> mlua::Result<Vec<u8>> {
    let mut buf = Vec::new();
    while let Some(x) = self.0.try_next().await? {
      buf.extend_from_slice(&x);
    }
    Ok(buf)
  }
}

impl From<Body> for ByteStream {
  fn from(body: Body) -> Self {
    Self(body.map_err(rt_error).boxed())
  }
}

impl UserData for ByteStream {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    // Added since a file may turn into `ByteStream`
    methods.add_meta_function("__close", |_lua, this: AnyUserData| {
      drop(this.take::<Self>());
      Ok(())
    });

    methods.add_async_function("to_string", |lua, mut args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(args.pop_front(), "byte stream")
        .map_err(tag_handler(lua, 1, 1))?;
      this
        .with_borrowed_mut(|x| x.aggregate())
        .await
        .map(|x| lua.pack_multi(lua.create_string(&x)?))
        .unwrap_or_else(|x| lua.pack_multi((Nil, x.to_string())))
    });

    methods.add_async_function("parse_json", |lua, mut args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(args.pop_front(), "byte stream")
        .map_err(tag_handler(lua, 1, 1))?;
      lua.pack_multi(
        async {
          let bytes = this.with_borrowed_mut(|x| x.aggregate()).await?;
          let v: serde_json::Value = serde_json::from_slice(&bytes).to_lua_err()?;
          lua.to_value(&v)
        }
        .await,
      )
    });
  }
}
