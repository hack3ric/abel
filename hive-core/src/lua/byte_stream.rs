use crate::Result;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use hyper::body::Bytes;
use mlua::{AnyUserData, ExternalResult, LuaSerdeExt, UserData, UserDataMethods};
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;

pub struct ByteStream(pub(crate) BoxStream<'static, Result<Bytes>>);

impl ByteStream {
  pub fn from_async_read(r: impl AsyncRead + Send + 'static) -> Self {
    Self(ReaderStream::new(r).map_err(crate::Error::from).boxed())
  }

  async fn aggregate(&mut self) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    while let Some(x) = self.0.try_next().await? {
      buf.extend_from_slice(&x);
    }
    Ok(buf)
  }
}

impl From<hyper::Body> for ByteStream {
  fn from(body: hyper::Body) -> Self {
    Self(body.map_err(crate::Error::from).boxed())
  }
}

impl UserData for ByteStream {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_function("to_string", |lua, this: AnyUserData| async move {
      let mut this = this.borrow_mut::<Self>()?;
      lua.create_string(&this.aggregate().await?)
    });

    methods.add_async_function("parse_json", |lua, this: AnyUserData| async move {
      let mut this = this.borrow_mut::<Self>()?;
      let bytes = this.aggregate().await?;
      let v: serde_json::Value = serde_json::from_slice(&bytes).to_lua_err()?;
      lua.to_value(&v)
    });
  }
}
