use crate::{Error, FileMode, Metadata, Result, Vfs};
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use std::io::ErrorKind;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::fs::{create_dir, metadata, read_dir, remove_dir, remove_file, File, OpenOptions};
use tokio::io::{self, AsyncSeek, AsyncRead};
use tokio_stream::wrappers::ReadDirStream;

pub struct FileSystem(());

#[async_trait]
impl Vfs for FileSystem {
  type File = File;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a,
  {
    let mut options = OpenOptions::new();
    match mode {
      FileMode::Read => options.read(true),
      FileMode::Write => options.create(true).truncate(true).write(true),
      FileMode::Append => options.create(true).append(true),
      FileMode::ReadWrite => options.read(true).write(true),
      FileMode::ReadWriteNew => options.create(true).truncate(true).read(true).write(true),
      FileMode::ReadAppend => options.create(true).read(true).append(true),
    };
    Ok(options.open(path).await?)
  }

  async fn read_dir(&self, path: &str) -> Result<BoxStream<Result<String>>> {
    Ok(
      ReadDirStream::new(read_dir(path).await?)
        .map_ok(|x| x.path().to_string_lossy().to_string())
        .map_err(From::from)
        .boxed(),
    )
  }

  async fn metadata(&self, path: &str) -> Result<Metadata> {
    let metadata = metadata(path).await?;
    if metadata.is_dir() {
      Ok(Metadata::Directory)
    } else if metadata.is_file() {
      Ok(Metadata::File {
        len: metadata.len(),
      })
    } else {
      // maybe symlink
      // TODO: change error type
      Err(Error::MethodNotAllowed)
    }
  }

  async fn exists(&self, path: &str) -> Result<bool> {
    match File::open(path).await {
      Ok(_) => Ok(true),
      Err(x) if matches!(x.kind(), ErrorKind::NotFound) => Ok(false),
      Err(x) => Err(Error::Io(x)),
    }
  }

  async fn create_dir(&self, path: &str) -> Result<()> {
    Ok(create_dir(path).await?)
  }

  async fn remove_file(&self, path: &str) -> Result<()> {
    Ok(remove_file(path).await?)
  }

  async fn remove_dir(&self, path: &str) -> Result<()> {
    Ok(remove_dir(path).await?)
  }
}

/// Wrapper type that makes an VFS read-only.
pub struct ReadOnlyVfs<T: Vfs + Send + Sync>(T);

#[async_trait]
impl<T: Vfs + Send + Sync> Vfs for ReadOnlyVfs<T> {
  type File = ReadOnly<T::File>;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> Result<Self::File>
  where
    Self::File: 'a,
  {
    if let FileMode::Read = mode {
      Ok(ReadOnly(self.0.open_file(path, mode).await?))
    } else {
      Err(Error::MethodNotAllowed)
    }
  }

  async fn read_dir(&self, path: &str) -> Result<BoxStream<Result<String>>> {
    self.0.read_dir(path).await
  }

  async fn metadata(&self, path: &str) -> Result<Metadata> {
    self.0.metadata(path).await
  }

  async fn exists(&self, path: &str) -> Result<bool> {
    self.0.exists(path).await
  }
}

pub struct ReadOnly<T: AsyncRead>(T);

impl<T: AsyncRead> ReadOnly<T> {
  fn pin_get_inner_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
    unsafe { self.map_unchecked_mut(|x| &mut x.0) }
  }
}

impl<T: AsyncRead> AsyncRead for ReadOnly<T> {
  fn poll_read(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut io::ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    self.pin_get_inner_mut().poll_read(cx, buf)
  }
}

impl<T: AsyncRead + AsyncSeek> AsyncSeek for ReadOnly<T> {
  fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
    self.pin_get_inner_mut().start_seek(position)
  }

  fn poll_complete(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
    self.pin_get_inner_mut().poll_complete(cx)
  }
}
