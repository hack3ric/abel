pub use regex::Error as RegexError;

use regex::Regex;
use std::collections::HashMap;
use std::lazy::SyncLazy;

static PATH_PARAMS_REGEX: SyncLazy<Regex> = SyncLazy::new(|| Regex::new(r":([^/]+)|\*").unwrap());

#[derive(Debug)]
pub struct PathMatcher {
  regex: Regex,
  param_names: Vec<Box<str>>,
}

impl PathMatcher {
  pub fn new(matcher: &str) -> Result<Self, RegexError> {
    let mut regex = "^".to_owned();
    let mut param_names = Vec::new();
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
    regex += "$";

    Ok(Self {
      regex: Regex::new(&regex)?,
      param_names,
    })
  }

  pub fn gen_params(&self, path: &str) -> Option<HashMap<Box<str>, Box<str>>> {
    self.regex.captures(path).map(|captures| {
      let mut params = HashMap::new();
      self
        .param_names
        .iter()
        .zip(captures.iter().skip(1))
        .for_each(|(name, match_)| {
          if let Some(match_) = match_ {
            params.insert(name.clone(), match_.as_str().into());
          }
        });
      params
    })
  }
}
