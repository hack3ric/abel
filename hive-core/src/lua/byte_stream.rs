use super::error::{check_userdata_mut, extract_error_async};
use crate::Result;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use hyper::body::Bytes;
use hyper::Body;
use mlua::{ExternalResult, LuaSerdeExt, MultiValue, UserData, UserDataMethods};
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

impl From<Body> for ByteStream {
  fn from(body: Body) -> Self {
    Self(body.map_err(crate::Error::from).boxed())
  }
}

impl UserData for ByteStream {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_function("to_string", |lua, args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "byte stream", 1)?;
      extract_error_async(lua, async { lua.create_string(&this.aggregate().await?) }).await
    });

    methods.add_async_function("parse_json", |lua, args: MultiValue| async move {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "byte stream", 1)?;
      extract_error_async(lua, async {
        let bytes = this.aggregate().await?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).to_lua_err()?;
        lua.to_value(&v)
      })
      .await
    });
  }
}
