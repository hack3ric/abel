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

#[cfg(test)]
mod tests {
  use super::*;
  use test_case::test_case;

  #[test_case("" => ""; "empty string")]
  #[test_case("etc/rpc" => "etc/rpc"; "force absolute")]
  #[test_case("../../././///etc/rpc" => "etc/rpc"; "special path components")]
  fn test_normalize_path_str(path: &str) -> String {
    normalize_path_str(path)
  }
}
