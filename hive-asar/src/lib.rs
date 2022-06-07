pub mod archive;
pub mod header;
pub mod writer;

pub use archive::{Archive, File, FileArchive};
pub use header::{Algorithm, Directory, Entry, FileMetadata, Integrity};
pub use writer::Writer;

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
