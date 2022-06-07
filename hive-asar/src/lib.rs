//! Asynchronous parser and writer for Electron's asar archive format.
//!
//! Requires Tokio runtime.

pub mod header;

mod archive;
mod writer;

pub use archive::{Archive, File, FileArchive};
pub use writer::{pack_dir, Writer};

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
