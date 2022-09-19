use crate::source::DirSource;
use abel_core::mlua::{ExternalResult, Lua, Table};
use abel_core::source::{Source, SourceUserData};
use abel_core::{load_create_require, mlua, RemoteInterface};
use data_encoding::HEXLOWER;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub async fn resolve_dep(path: PathBuf) -> mlua::Result<()> {
  let lua = Lua::new();
  let create_require = load_create_require(&lua)?;
  let source = Source::new(DirSource(path));
  let remote = RemoteInterface::new(None);
  let sha256 = lua.create_function(|lua, s: mlua::String| {
    let out = HEXLOWER.encode(&Sha256::digest(s));
    lua.create_string(&out)
  })?;
  let hashes: Table = lua
    .load(include_str!("resolve_dep.lua"))
    .call_async((SourceUserData(source), remote, create_require, sha256))
    .await?;
  println!("{}", serde_json::to_string_pretty(&hashes).to_lua_err()?);
  Ok(())
}
