use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
  File(FileMetadata),
  Directory(Directory),
}

impl Entry {
  pub(crate) fn search_segments(&self, segments: &[&str]) -> Option<&Entry> {
    match self {
      _ if segments.is_empty() => Some(self),
      Self::File(_) => None,
      Self::Directory(dir) => dir.search_segments(segments),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
  #[serde(with = "serde_offset")]
  pub offset: u64,
  // no larger than 9007199254740991
  pub size: u64,
  #[serde(default)]
  pub executable: bool,
  pub integrity: Option<Integrity>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Directory {
  files: HashMap<Box<str>, Entry>,
}

impl Directory {
  pub(crate) fn search_segments(&self, segments: &[&str]) -> Option<&Entry> {
    (self.files)
      .get(segments[0])
      .and_then(|x| x.search_segments(&segments[1..]))
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Integrity {
  pub algorithm: Algorithm,
  pub hash: String,
  #[serde(rename = "blockSize")]
  pub block_size: u32,
  pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Algorithm {
  SHA256,
}

mod serde_offset {
  use serde::de::Error;
  use serde::{Deserialize, Deserializer, Serializer};

  pub fn serialize<S: Serializer>(offset: &u64, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&offset.to_string())
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<u64, D::Error> {
    u64::from_str_radix(&String::deserialize(de)?, 10).map_err(D::Error::custom)
  }
}
