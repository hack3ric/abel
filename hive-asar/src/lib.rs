mod archive;
mod header;
mod writer;

pub use archive::{Archive, File};
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
