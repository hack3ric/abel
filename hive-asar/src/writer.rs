use crate::{split_path, Directory, Entry, FileMetadata};
use tokio::io::{self, AsyncRead, AsyncWrite, AsyncWriteExt};

#[derive(Debug)]
pub struct Writer<F: AsyncRead + Unpin> {
  header: Directory,
  file_offset: u64,
  files: Vec<F>,
}

impl<F: AsyncRead + Unpin> Writer<F> {
  pub fn new() -> Self {
    Default::default()
  }

  fn add_folder_recursively(&mut self, segments: Vec<&str>) -> &mut Directory {
    let mut dir = &mut self.header;
    for seg in segments {
      dir = if let Entry::Directory(dir) = dir
        .files
        .entry(seg.into())
        .or_insert_with(|| Entry::Directory(Default::default()))
      {
        dir
      } else {
        unreachable!();
      }
    }
    dir
  }

  pub fn add(&mut self, path: &str, content: F, size: u64) {
    let mut segments = split_path(path);
    let filename = segments.pop().unwrap(); // TODO: handle unwrap
    let file_entry = FileMetadata {
      offset: self.file_offset,
      size,
      executable: false,
      integrity: None,
    };
    let result = self
      .add_folder_recursively(segments)
      .files
      .insert(filename.into(), Entry::File(file_entry));
    assert!(result.is_none()); // TODO: handle duplicate
    self.file_offset += size;
    self.files.push(content)
  }

  pub fn add_empty_folder(&mut self, path: &str) {
    self.add_folder_recursively(split_path(path));
  }

  pub async fn write(self, writer: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let header_bytes = serde_json::to_vec(&self.header).unwrap();
    let header_len = header_bytes.len() as u32;
    let padding = match header_len % 4 {
      0 => 0,
      r => 4 - r,
    };

    writer.write_u32_le(4).await?;
    writer.write_u32_le(header_len + padding + 8).await?;
    writer.write_u32_le(header_len + padding + 4).await?;
    writer.write_u32_le(header_len).await?;

    writer.write_all(&header_bytes).await?;
    writer.write_all(&vec![0; padding as _]).await?;

    for mut file in self.files {
      io::copy(&mut file, writer).await?;
    }

    Ok(())
  }
}

impl<F: AsyncRead + Unpin> Default for Writer<F> {
  fn default() -> Self {
    Self {
      header: Default::default(),
      file_offset: 0,
      files: Vec::new(),
    }
  }
}
