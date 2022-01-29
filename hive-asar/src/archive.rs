use crate::header::{Directory, Entry, FileMetadata};
use crate::split_path;
use std::io::SeekFrom;
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
