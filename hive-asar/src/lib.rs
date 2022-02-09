mod archive;
mod header;

pub use archive::{Archive, File};
pub use header::{Algorithm, Directory, Entry, FileMetadata, Integrity};

#[cfg(feature = "vfs")]
mod file_archive;
#[cfg(feature = "vfs")]
pub use file_archive::FileArchive;

pub(crate) fn split_path(path: &str) -> Vec<&str> {
  path
    .split('/')
    .filter(|x| !x.is_empty() && *x != ".")
    .fold(Vec::new(), |mut result, segment| {
      if segment == ".." {
        result.pop();
      } else {
        result.push(segment);
      }
      result
    })
}
