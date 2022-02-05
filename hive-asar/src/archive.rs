use crate::header::{Directory, Entry, FileMetadata};
use crate::split_path;
use std::future::Future;
use std::io::SeekFrom;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::fs;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, Take};

pub struct Archive<R: AsyncRead + AsyncSeek + Unpin> {
  offset: u64,
  header: Directory,
  reader: R,
}

impl<R: AsyncRead + AsyncSeek + Unpin> Archive<R> {
  pub async fn new(mut reader: R) -> io::Result<Self> {
    reader.seek(SeekFrom::Start(12)).await?;
    let header_size = dbg!(reader.read_u32_le().await?);

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

  pub async fn read(&mut self, path: &str) -> io::Result<File<&mut R>> {
    let entry = self.header.search_segments(&split_path(path));
    match entry {
      Some(Entry::File(metadata)) => {
        (self.reader)
          .seek(SeekFrom::Start(self.offset + metadata.offset))
          .await?;
        Ok(File {
          metadata: metadata.clone(),
          content: (&mut self.reader).take(metadata.size),
        })
      }
      Some(_) => Err(io::Error::new(io::ErrorKind::Other, "not a file")),
      None => Err(io::ErrorKind::NotFound.into()),
    }
  }

  pub async fn extract(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    for (name, entry) in self.header.files.iter() {
      extract_entry(&mut self.reader, self.offset, name, entry, path).await?;
    }
    Ok(())
  }
}

fn extract_entry<'a, R: AsyncRead + AsyncSeek + Unpin>(
  reader: &'a mut R,
  offset: u64,
  name: &'a str,
  entry: &'a Entry,
  path: &'a Path,
) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'a>> {
  Box::pin(async move {
    match entry {
      Entry::File(file) => extract_file(reader, offset, name, file, path).await?,
      Entry::Directory(dir) => extract_dir(reader, offset, name, dir, path).await?,
    }
    Ok(())
  })
}

async fn extract_file<R: AsyncRead + AsyncSeek + Unpin>(
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

async fn extract_dir<R: AsyncRead + AsyncSeek + Unpin>(
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

impl Archive<fs::File> {
  pub async fn fs_read(&self, path: &str) -> io::Result<File<fs::File>> {
    let entry = self.header.search_segments(&split_path(path));
    match entry {
      Some(Entry::File(metadata)) => {
        let mut r = self.reader.try_clone().await?;
        r.seek(SeekFrom::Start(self.offset + metadata.offset))
          .await?;
        Ok(File {
          metadata: metadata.clone(),
          content: r.take(metadata.size),
        })
      }
      Some(_) => Err(io::Error::new(io::ErrorKind::Other, "not a file")),
      None => Err(io::ErrorKind::NotFound.into()),
    }
  }
}

pub struct File<R: AsyncRead + AsyncSeek + Unpin> {
  metadata: FileMetadata,
  content: Take<R>,
}

impl<R: AsyncRead + AsyncSeek + Unpin> File<R> {
  pub fn metadata(&self) -> &FileMetadata {
    &self.metadata
  }
}

impl<R: AsyncRead + AsyncSeek + Unpin> AsyncRead for File<R> {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut io::ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    Pin::new(&mut self.content).poll_read(cx, buf)
  }
}

// TODO: impl AsyncSeek for File