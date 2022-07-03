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
#[clap(author, version, about)]
pub struct Args {
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
  pub auth_token: Uuid,
  pool_size: Option<usize>,
}

impl Config {
  async fn init(path: impl AsRef<Path>) -> io::Result<Config> {
    let default_config = Self {
      listen: ([127, 0, 0, 1], 3000).into(),
      auth_token: Uuid::new_v4(),
      pool_size: None,
    };
    let content = serde_json::to_string_pretty(&default_config)?;
    fs::write(path, content.as_bytes()).await?;
    Ok(default_config)
  }

  pub async fn get(config_path: impl AsRef<Path>, args: ConfigArgs) -> io::Result<Config> {
    let config_path = config_path.as_ref();
    let mut config = if !config_path.exists() {
      Config::init(config_path).await?
    } else {
      let content = fs::read(config_path).await?;
      serde_json::from_slice(&content)?
    };

    // merge
    #[allow(clippy::option_map_unit_fn)]
    {
      args.listen.map(|x| config.listen = x);
      args.auth_token.map(|x| config.auth_token = x);
      args.pool_size.map(|x| config.pool_size = Some(x));
    }

    Ok(config)
  }

  pub fn pool_size(&self) -> usize {
    self.pool_size.unwrap_or(*HALF_NUM_CPUS)
  }
}
