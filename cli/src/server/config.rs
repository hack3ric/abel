use clap::Parser;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

pub static HALF_NUM_CPUS: Lazy<usize> = Lazy::new(|| 1.max(num_cpus::get() / 2));

#[derive(Debug, Parser)]
#[clap(version)]
pub struct ServerArgs {
  #[clap(flatten)]
  pub config: ConfigArgs,

  /// Abel's working path.
  #[clap(long, default_value_os_t = get_default_abel_path())]
  pub abel_path: PathBuf,
}

fn get_default_abel_path() -> PathBuf {
  let mut abel_path = home::home_dir().expect("no home directory found");
  abel_path.push(".abel");
  abel_path
}

#[derive(Debug, Clone, Parser)]
#[clap(author, version, about)]
pub struct ConfigArgs {
  /// Listening address [overrides config]
  #[clap(short, long)]
  pub listen: Option<SocketAddr>,

  /// Authentication token [overrides config]
  #[clap(long)]
  pub auth_token: Option<Uuid>,

  /// Abel executor pool size [overrides config]
  #[clap(long)]
  pub pool_size: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
  pub listen: SocketAddr,
  pub auth_token: Option<Uuid>,
  pub(crate) pool_size: Option<usize>,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      listen: ([127, 0, 0, 1], 3000).into(),
      auth_token: Some(Uuid::new_v4()),
      pool_size: None,
    }
  }
}

impl Config {
  async fn init(path: impl AsRef<Path>) -> io::Result<Self> {
    let default_config = Self::default();
    let content = serde_json::to_string_pretty(&default_config)?;
    fs::write(path, content.as_bytes()).await?;
    Ok(default_config)
  }

  pub async fn load(path: impl AsRef<Path>) -> io::Result<Self> {
    let path = path.as_ref();
    let config = if !path.exists() {
      Config::init(path).await?
    } else {
      let content = fs::read(path).await?;
      serde_json::from_slice(&content)?
    };

    Ok(config)
  }

  #[allow(clippy::option_map_unit_fn)]
  pub fn merge(mut self, args: ConfigArgs) -> Self {
    args.listen.map(|x| self.listen = x);
    args.auth_token.map(|x| self.auth_token = Some(x));
    args.pool_size.map(|x| self.pool_size = Some(x));
    self
  }

  pub fn pool_size(&self) -> usize {
    self.pool_size.unwrap_or(*HALF_NUM_CPUS)
  }
}
