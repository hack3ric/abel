use crate::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::ser::SerializeStruct;
use serde::Serialize;
use std::collections::HashMap;

static PATH_PARAMS_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r":([^/]+)|\*").unwrap());

pub type Params = HashMap<Box<str>, Box<str>>;

#[derive(Debug, Clone)]
pub struct PathMatcher {
  path: Box<str>,
  regex: Regex,
  param_names: Vec<Box<str>>,
}

impl PathMatcher {
  pub fn new(matcher: &str) -> Result<Self> {
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

impl Serialize for PathMatcher {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let mut x = serializer.serialize_struct("PathMatcher", 2)?;
    x.serialize_field("pattern", self.as_str())?;
    x.serialize_field("regex", self.as_regex_str())?;
    x.end()
  }
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
}
