use std::collections::HashMap;

pub type Params = HashMap<Box<str>, Box<str>>;

/// Similar to `abel_core::path::normalize_path`, but for `str`s instead of
/// `Path`s.
///
/// The returned path is always relative, which is intentional and convenient
/// for concatenating to other paths in usual cases.
pub fn normalize_path_str(path: &str) -> String {
  let mut result = Vec::new();
  let segments = path
    .split(['/', '\\'])
    .filter(|&x| !x.is_empty() && x != ".");
  for s in segments {
    if s == ".." {
      result.pop();
    } else {
      result.push(s);
    }
  }
  result.join("/")
}
