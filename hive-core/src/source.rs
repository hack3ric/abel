use std::sync::Arc;

/// Shared, immutable source code storage.
#[derive(Debug, Clone)]
pub enum Source {
  Single(Arc<str>), // Multiple(Archive)
}

impl Source {
  pub fn get(&self, path: &str) -> Option<&[u8]> {
    let segments: Vec<_> = path.split("/").filter(|x| !x.is_empty()).collect();
    match self {
      Self::Single(main) if segments.len() == 1 && segments[0] == "main.lua" => {
        Some(main.as_bytes())
      }
      _ => None,
    }
  }
}
