use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
  #[serde(rename = "name")]
  pub pkg_name: Option<String>,
  pub description: Option<String>,
}
