use std::sync::Arc;

/// Shared, immutable source code storage.
#[derive(Debug, Clone)]
pub struct Source {
  inner: SourceInner,
}

#[derive(Debug, Clone)]
enum SourceInner {
  Single(Arc<[u8]>),
}

impl Source {
  pub fn new_single(content: impl Into<Arc<[u8]>>) -> Self {
    Self {
      inner: SourceInner::Single(content.into()),
    }
  }

  pub(crate) fn get(&self, path: &str) -> Option<&[u8]> {
    let segments: Vec<_> = path.split("/").filter(|x| !x.is_empty()).collect();
    match &self.inner {
      SourceInner::Single(main) if segments.len() == 1 && segments[0] == "main.lua" => Some(&main),
      _ => None,
    }
  }
}
