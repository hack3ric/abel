use crate::{split_path, Archive, Entry, File};
use async_trait::async_trait;
use futures::stream::BoxStream;
use hive_vfs::{FileMode, Metadata, Vfs};
use std::io::SeekFrom;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::{fs, io};

/// File-based ASAR archive, mainly used for VFS implementation.
#[derive(Debug)]
pub struct FileArchive {
  path: PathBuf,
  archive: Archive<fs::File>,
}

impl FileArchive {
  pub async fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
    let path = path.into();
    let archive = Archive::new(fs::File::open(&path).await?).await?;
    Ok(Self { path, archive })
  }

  async fn read(&self, path: &str) -> hive_vfs::Result<File<fs::File>> {
    let entry = self.archive.header.search_segments(&split_path(path));
    match entry {
      Some(Entry::File(metadata)) => {
        let mut r = fs::File::open(&self.path).await?;
        r.seek(SeekFrom::Start(self.archive.offset + metadata.offset))
          .await?;
        Ok(File {
          metadata: metadata.clone(),
          content: r.take(metadata.size),
        })
      }
      Some(_) => Err(hive_vfs::Error::IsADirectory(path.into())),
      None => Err(hive_vfs::Error::NotFound(path.into())),
    }
  }
}

#[async_trait]
impl Vfs for FileArchive {
  type File = File<fs::File>;

  async fn open_file<'a>(&'a self, path: &str, mode: FileMode) -> hive_vfs::Result<Self::File>
  where
    Self::File: 'a,
  {
    if let FileMode::Read = mode {
      Ok(self.read(path).await?)
    } else {
      Err(hive_vfs::Error::MethodNotAllowed)
    }
  }

  async fn read_dir(&self, path: &str) -> hive_vfs::Result<BoxStream<hive_vfs::Result<String>>> {
    let segments = split_path(path);
    let entry = self.archive.header.search_segments(&segments);
    match entry {
      Some(Entry::Directory(d)) => {
        let x = (d.files.iter())
          .map(|(name, _entry)| {
            let result = segments
              .iter()
              .copied()
              .chain(std::iter::once(&**name))
              .fold(String::new(), |b, x| b + "/" + x);
            Ok(result)
          })
          .collect::<Vec<_>>();
        Ok(Box::pin(futures::stream::iter(x)))
      }
      Some(_) => Err(hive_vfs::Error::NotADirectory(path.into())),
      None => Err(hive_vfs::Error::NotFound(path.into())),
    }
  }

  async fn metadata(&self, path: &str) -> hive_vfs::Result<Metadata> {
    let entry = self.archive.header.search_segments(&split_path(path));
    match entry {
      Some(Entry::Directory(_)) => Ok(Metadata::Directory),
      Some(Entry::File(f)) => Ok(Metadata::File { len: f.size }),
      None => Err(hive_vfs::Error::NotFound(path.into())),
    }
  }

  async fn exists(&self, path: &str) -> hive_vfs::Result<bool> {
    let entry = self.archive.header.search_segments(&split_path(path));
    Ok(entry.is_some())
  }

  async fn create_dir(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }

  async fn remove_file(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }

  async fn remove_dir(&self, _path: &str) -> hive_vfs::Result<()> {
    Err(hive_vfs::Error::MethodNotAllowed)
  }
}