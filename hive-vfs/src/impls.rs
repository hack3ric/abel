use crate::{normalize_path, Error, FileMode, Metadata, Result, ResultExt, Vfs};
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::fs::{
  canonicalize, create_dir, metadata, read_dir, remove_dir, remove_file, File, OpenOptions,
};
use tokio::io::{self, AsyncRead, AsyncSeek};
use tokio_stream::wrappers::ReadDirStream;

/// The file system, based on a directory as its root.
#[derive(Debug)]
pub struct FileSystem {
  root: PathBuf,
}

impl FileSystem {
  pub async fn new(root: impl AsRef<Path>) -> io::Result<Self> {
    Ok(Self {
      root: canonicalize(root).await?,
    })
  }

  async fn real_path(&self, path: &str) -> io::Result<PathBuf> {
    let path = normalize_path(path);
    let result = canonicalize(self.root.join(path)).await?;
    if result.starts_with(&self.root) {
      Ok(result)
    } else {
      Err(io::ErrorKind::PermissionDenied.into())
    }
  }
}

#[async_trait]
impl Vfs for FileSystem {
  type File = File;

  async fn open_file(&self, path: &str, mode: FileMode) -> Result<Self::File> {
    let real_path = self.real_path(path).await?;
    let mut options = OpenOptions::new();
    match mode {
      FileMode::Read => options.read(true),
      FileMode::Write => options.create(true).truncate(true).write(true),
      FileMode::Append => options.create(true).append(true),
      FileMode::ReadWrite => options.read(true).write(true),
      FileMode::ReadWriteNew => options.create(true).truncate(true).read(true).write(true),
      FileMode::ReadAppend => options.create(true).read(true).append(true),
    };
    options.open(real_path).await.to_vfs_err(path)
  }

  async fn read_dir(&self, path: &str) -> Result<BoxStream<Result<String>>> {
    let real_path = self.real_path(path).await?;

    Ok(
      ReadDirStream::new(read_dir(real_path).await.to_vfs_err(path)?)
        .map_ok(|x| x.path().to_string_lossy().to_string())
        .map_err(From::from)
        .boxed(),
    )
  }

  async fn metadata(&self, path: &str) -> Result<Metadata> {
    let real_path = self.real_path(path).await?;
    let metadata = metadata(real_path).await?;
    if metadata.is_dir() {
      Ok(Metadata::Directory)
    } else if metadata.is_file() {
      Ok(Metadata::File {
        len: metadata.len(),
      })
    } else {
      // maybe symlink
      // TODO: change error type
      Err(Error::NotFound(path.into()))
    }
  }

  async fn exists(&self, path: &str) -> Result<bool> {
    let path = self.real_path(path).await?;
    match File::open(path).await {
      Ok(_) => Ok(true),
      Err(x) if matches!(x.kind(), ErrorKind::NotFound) => Ok(false),
      Err(x) => Err(Error::Io(x)),
    }
  }

  async fn create_dir(&self, path: &str) -> Result<()> {
    let path = self.real_path(path).await?;
    Ok(create_dir(path).await?)
  }

  async fn remove_file(&self, path: &str) -> Result<()> {
    let path = self.real_path(path).await?;
    Ok(remove_file(path).await?)
  }

  async fn remove_dir(&self, path: &str) -> Result<()> {
    let path = self.real_path(path).await?;
    Ok(remove_dir(path).await?)
  }
}

/// Wrapper type that makes a VFS read-only.
pub struct ReadOnlyVfs<T: Vfs + Send + Sync>(T);

#[async_trait]
impl<T: Vfs + Send + Sync> Vfs for ReadOnlyVfs<T> {
  type File = ReadOnly<T::File>;

  async fn open_file(&self, path: &str, mode: FileMode) -> Result<Self::File> {
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

  async fn create_dir(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn remove_file(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, _path: &str) -> Result<()> {
    Err(Error::MethodNotAllowed)
  }
}

/// Wrapper type that makes an `AsyncRead + AsyncWrite` object read-only.
///
/// Implements `AsyncSeek` if the inner object does so.
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
