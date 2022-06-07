use crate::header::{Directory, Entry, FileMetadata};
use crate::split_path;
use std::future::Future;
use std::io::SeekFrom;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::fs::{self, File as TokioFile};
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, Take};

/// ASAR archive reader.
#[derive(Debug)]
pub struct Archive<R: AsyncRead + AsyncSeek + Send + Sync + Unpin> {
  pub(crate) offset: u64,
  pub(crate) header: Directory,
  pub(crate) reader: R,
}

impl<R: AsyncRead + AsyncSeek + Send + Sync + Unpin> Archive<R> {
  /// Parses an ASAR archive into `Archive`.
  pub async fn new(mut reader: R) -> io::Result<Self> {
    reader.seek(SeekFrom::Start(12)).await?;
    let header_size = reader.read_u32_le().await?;

    let mut header_bytes = vec![0; header_size as _];
    reader.read_exact(&mut header_bytes).await?;

    let header = serde_json::from_slice(&header_bytes).unwrap();
    let offset = match header_size % 4 {
      0 => header_size + 16,
      r => header_size + 16 + 4 - r,
    } as u64;

    Ok(Self {
      offset,
      header,
      reader,
    })
  }

  /// Reads a file entry from the archive.
  pub async fn read(&mut self, path: &str) -> io::Result<File<&mut R>> {
    let entry = self.header.search_segments(&split_path(path));
    match entry {
      Some(Entry::File(metadata)) => {
        (self.reader)
          .seek(SeekFrom::Start(self.offset + metadata.offset))
          .await?;
        Ok(File {
          offset: self.offset,
          metadata: metadata.clone(),
          content: (&mut self.reader).take(metadata.size),
        })
      }
      Some(_) => Err(io::Error::new(io::ErrorKind::Other, "not a file")),
      None => Err(io::ErrorKind::NotFound.into()),
    }
  }

  /// Extracts the archive to a folder.
  pub async fn extract(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    for (name, entry) in self.header.files.iter() {
      extract_entry(&mut self.reader, self.offset, name, entry, path).await?;
    }
    Ok(())
  }
}

/// File-based ASAR archive reader, allowing multiple read access at a time
/// through `read_owned`.
///
/// It implements `Deref<Target = Archive>`, so `Archive`'s methods can still be
/// used.
#[derive(Debug)]
pub struct FileArchive {
  archive: Archive<TokioFile>,
  path: PathBuf,
}

impl FileArchive {
  /// Parses an ASAR archive into `FileArchive`.
  pub async fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
    let path = path.into();
    let file = TokioFile::open(&path).await?;
    Ok(Self {
      archive: Archive::new(file).await?,
      path,
    })
  }

  /// Reads a file entry from the archive.
  ///
  /// Contrary to `Archive::read`, it allows multiple read access over a single
  /// archive by creating a new file handle for every file.
  pub async fn read_owned(&self, path: &str) -> io::Result<File<TokioFile>> {
    let entry = self.archive.header.search_segments(&split_path(path));
    match entry {
      Some(Entry::File(metadata)) => {
        let mut file = TokioFile::open(&self.path).await?;
        let seek_from = SeekFrom::Start(self.archive.offset + metadata.offset);
        file.seek(seek_from).await?;
        Ok(File {
          offset: self.offset,
          metadata: metadata.clone(),
          content: file.take(metadata.size),
        })
      }
      Some(_) => Err(io::Error::new(io::ErrorKind::Other, "not a file")),
      None => Err(io::ErrorKind::NotFound.into()),
    }
  }
}

impl Deref for FileArchive {
  type Target = Archive<TokioFile>;

  fn deref(&self) -> &Self::Target {
    &self.archive
  }
}

impl DerefMut for FileArchive {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.archive
  }
}

fn extract_entry<'a, R: AsyncRead + AsyncSeek + Send + Sync + Unpin>(
  reader: &'a mut R,
  offset: u64,
  name: &'a str,
  entry: &'a Entry,
  path: &'a Path,
) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + Sync + 'a>> {
  Box::pin(async move {
    match entry {
      Entry::File(file) => extract_file(reader, offset, name, file, path).await?,
      Entry::Directory(dir) => extract_dir(reader, offset, name, dir, path).await?,
    }
    Ok(())
  })
}

async fn extract_file<R: AsyncRead + AsyncSeek + Send + Sync + Unpin>(
  reader: &mut R,
  offset: u64,
  name: &str,
  file: &FileMetadata,
  path: &Path,
) -> io::Result<()> {
  reader.seek(SeekFrom::Start(offset + file.offset)).await?;
  let mut dest = fs::File::create(path.join(name)).await?;
  io::copy(&mut reader.take(file.size), &mut dest).await?;
  Ok(())
}

async fn extract_dir<R: AsyncRead + AsyncSeek + Send + Sync + Unpin>(
  reader: &mut R,
  offset: u64,
  name: &str,
  dir: &Directory,
  path: &Path,
) -> io::Result<()> {
  let new_dir_path = path.join(name);
  fs::create_dir(&new_dir_path).await?;
  for (name, entry) in dir.files.iter() {
    extract_entry(reader, offset, name, entry, &new_dir_path).await?;
  }
  Ok(())
}

/// File from an ASAR archive.
pub struct File<R: AsyncRead + AsyncSeek + Send + Sync + Unpin> {
  offset: u64,
  pub(crate) metadata: FileMetadata,
  pub(crate) content: Take<R>,
}

impl<R: AsyncRead + AsyncSeek + Send + Sync + Unpin> File<R> {
  /// Gets the metadata of the file.
  pub fn metadata(&self) -> &FileMetadata {
    &self.metadata
  }
}

impl<R: AsyncRead + AsyncSeek + Send + Sync + Unpin> AsyncRead for File<R> {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut io::ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    Pin::new(&mut self.content).poll_read(cx, buf)
  }
}

impl<R: AsyncRead + AsyncSeek + Send + Sync + Unpin> AsyncSeek for File<R> {
  fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
    let current_relative_pos = self.metadata.size - self.content.limit();
    let offset = self.offset + self.metadata.offset;
    let absolute_pos = match position {
      SeekFrom::Start(pos) => SeekFrom::Start(offset + self.metadata.size.min(pos)),
      SeekFrom::Current(pos) if -pos as u64 > current_relative_pos => {
        return Err(io::ErrorKind::InvalidInput.into())
      }
      SeekFrom::Current(pos) => {
        let relative_pos = pos.min((self.metadata.size - current_relative_pos) as i64);
        SeekFrom::Current(relative_pos)
      }
      SeekFrom::End(pos) if pos > 0 => SeekFrom::Start(offset + self.metadata.size),
      SeekFrom::End(pos) if -pos as u64 > self.metadata.size => {
        return Err(io::ErrorKind::InvalidInput.into())
      }
      SeekFrom::End(pos) => SeekFrom::Start(offset + self.metadata.size - (-pos as u64)),
    };
    Pin::new(self.content.get_mut()).start_seek(absolute_pos)
  }

  fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
    let result = Pin::new(self.content.get_mut()).poll_complete(cx);
    match result {
      Poll::Ready(Ok(result)) => {
        let new_relative_pos = result - self.offset - self.metadata.offset;
        let new_limit = self.metadata.size - new_relative_pos;
        self.content.set_limit(new_limit);
        Poll::Ready(Ok(new_relative_pos))
      }
      other => other,
    }
  }
}
