mod archive;
mod file_archive;
mod header;

pub use archive::{Archive, File};
pub use file_archive::FileArchive;
pub use header::{Algorithm, Directory, Entry, FileMetadata, Integrity};

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
