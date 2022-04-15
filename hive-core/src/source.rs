use crate::path::normalize_path_str;
use crate::Result;
use mlua::{ExternalResult, Function, Lua, Table, UserData};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Source {
  base: Arc<RwLock<PathBuf>>,
}

// TODO: maybe use sync fs?
impl Source {
  pub async fn new(base: impl AsRef<Path>) -> Result<Self> {
    let base = fs::canonicalize(base).await?;
    Ok(Self {
      base: Arc::new(RwLock::new(base)),
    })
  }

  pub async fn get(&self, path: &str) -> Result<fs::File> {
    let path = normalize_path_str(path);
    Ok(fs::File::open(self.base.read().await.join(path)).await?)
  }

  pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
    let mut code_file = self.get(path).await?;
    let mut code = if let Ok(metadata) = code_file.metadata().await {
      Vec::with_capacity(metadata.len() as _)
    } else {
      Vec::new()
    };
    code_file.read_to_end(&mut code).await?;
    Ok(code)
  }

  pub async fn exists(&self, path: &str) -> bool {
    (self.base.read().await)
      .join(normalize_path_str(path))
      .exists()
  }

  pub async fn rename_base(&self, new_path: PathBuf) -> Result<()> {
    let mut base = self.base.write().await;
    fs::rename(&*base, &new_path).await?;
    *base = new_path;
    Ok(())
  }

  pub(crate) async fn load<'lua>(
    &self,
    lua: &'lua Lua,
    path: &str,
    env: Table<'lua>,
  ) -> Result<Function<'lua>> {
    let code = self.get_bytes(path).await?;
    let result = lua
      .load(&code)
      .set_name(&format!("source:{path}"))?
      .set_environment(env)?
      .into_function()?;
    Ok(result)
  }
}

impl UserData for Source {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_method("exists", |_lua, this, path: mlua::String| async move {
      let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
      Ok(this.exists(path).await)
    });

    methods.add_async_method(
      "load",
      |lua, this, (path, env): (mlua::String, Table)| async move {
        let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
        this.load(lua, path, env).await.to_lua_err()
      },
    )
  }
}
