use crate::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Params = HashMap<Box<str>, Box<str>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMatcher {
  path: Box<str>,
  #[serde(with = "serde_regex")]
  regex: Regex,
  param_names: Vec<Box<str>>,
}

impl PathMatcher {
  pub fn new(matcher: &str) -> Result<Self> {
    static PATH_PARAMS_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r":([^/]+)|\*").unwrap());

    let mut regex = "^".to_owned();
    let mut param_names = Vec::new();

    if !matcher.starts_with('/') {
      regex += "/";
    }

    let mut start_pos = 0;
    for captures in PATH_PARAMS_REGEX.captures_iter(matcher) {
      let whole = captures.get(0).unwrap();
      regex += &regex::escape(&matcher[start_pos..whole.start()]);
      if whole.as_str() == "*" {
        regex += r"(.*)";
        param_names.push("*".into())
      } else {
        regex += r"([^/]+)";
        param_names.push(captures[1].into());
      }
      start_pos = whole.end();
    }
    regex += &regex::escape(&matcher[start_pos..]);
    regex += "$";

    Ok(Self {
      path: matcher.into(),
      regex: Regex::new(&regex)?,
      param_names,
    })
  }

  pub fn gen_params(&self, path: &str) -> Option<Params> {
    self.regex.captures(path).map(|captures| {
      self
        .param_names
        .iter()
        .zip(captures.iter().skip(1))
        .filter_map(|(n, m)| m.map(|m| (n.clone(), m.as_str().into())))
        .collect()
    })
  }

  pub fn as_str(&self) -> &str {
    &self.path
  }

  pub fn as_regex_str(&self) -> &str {
    self.regex.as_str()
  }
}

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

  macro_rules! some_map {
    ($($key:expr => $val:expr),*$(,)?) => ({
      let mut map = HashMap::new();
      $( map.insert($key.into(), $val.into()); )*
      Some(map)
    });
  }

  #[test_case("/hello/:name", "/hello/world" => some_map!("name" => "world"); "single param")]
  #[test_case("/hello/:name", "/hello/world/" => None; "trailing slash")]
  #[test_case("/files/*", "/files/path/to/secret/file" => some_map!("*" => "path/to/secret/file"); "asterisk")]
  fn test_path_matcher(matcher: &str, path: &str) -> Option<Params> {
    PathMatcher::new(matcher).unwrap().gen_params(path)
  }

  #[test_case("" => ""; "empty string")]
  #[test_case("etc/rpc" => "etc/rpc"; "force absolute")]
  #[test_case("../../././///etc/rpc" => "etc/rpc"; "special path components")]
  fn test_normalize_path_str(path: &str) -> String {
    normalize_path_str(path)
  }
}
