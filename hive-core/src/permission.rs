use crate::path::normalize_path;
use mlua::{ExternalError, ExternalResult, FromLua, Lua, String as LuaString, ToLua};
use std::collections::HashSet;
use std::fmt::{Display, Write};
use std::num::{NonZeroU16, ParseIntError};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Permission(PermissionInner);

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PermissionInner {
  Read(PathBuf),
  Write(PathBuf),
  Net(Host),
}

impl Permission {
  pub fn read(path: impl AsRef<Path>) -> Self {
    let path = normalize_path(path);
    Self(PermissionInner::Read(path))
  }

  pub fn write(path: impl AsRef<Path>) -> Self {
    let path = normalize_path(path);
    Self(PermissionInner::Write(path))
  }

  pub fn net(host: impl Into<String>) -> Result<Self, ParseIntError> {
    Ok(Self(PermissionInner::Net(Host::new(host.into())?)))
  }

  pub fn is_subset(&self, other: &Self) -> bool {
    use PermissionInner::*;
    match (&self.0, &other.0) {
      (Read(p1), Read(p2)) | (Write(p1), Write(p2)) => p1.starts_with(p2),
      (Net(h1), Net(h2)) => match (&h2.port, &h1.port) {
        _ if h1.host != h2.host => false,
        (None, _) => true,
        (Some(p1), Some(p2)) => p1 == p2,
        (Some(_), None) => false,
      },
      _ => false,
    }
  }

  pub fn is_superset(&self, other: &Self) -> bool {
    use PermissionInner::*;
    match (&self.0, &other.0) {
      (Read(p1), Read(p2)) | (Write(p1), Write(p2)) => p2.starts_with(p1),
      (Net(h1), Net(h2)) => match (&h1.port, &h2.port) {
        _ if h1.host != h2.host => false,
        (None, _) => true,
        (Some(p1), Some(p2)) => p1 == p2,
        (Some(_), None) => false,
      },
      _ => false,
    }
  }

  pub fn inner(&self) -> &PermissionInner {
    &self.0
  }

  pub fn name(&self) -> &'static str {
    use PermissionInner::*;
    match self.0 {
      Read(_) => "read",
      Write(_) => "write",
      Net(_) => "net",
    }
  }
}

impl<'lua> FromLua<'lua> for Permission {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    if let mlua::Value::Table(table) = lua_value {
      let name: LuaString = table.raw_get("name")?;
      match name.as_bytes() {
        x @ b"read" | x @ b"write" => {
          let path: LuaString = table.raw_get("path")?;
          let path = std::str::from_utf8(path.as_bytes()).to_lua_err()?;
          let path = Path::new(path);
          if x == b"read" {
            Ok(Self::read(path))
          } else {
            Ok(Self::write(path))
          }
        }
        b"net" => {
          let host: String = table.raw_get("string")?;
          Self::net(host).to_lua_err()
        }
        _ => Err("invalid permission name".to_lua_err()),
      }
    } else {
      Err("failed to parse `Permission`: expected table".to_lua_err())
    }
  }
}

impl<'lua> ToLua<'lua> for Permission {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    use PermissionInner::*;
    let table = match self.0 {
      Read(path) => lua.create_table_from([("name", "read"), ("path", &path.to_string_lossy())])?,
      Write(path) => {
        lua.create_table_from([("name", "write"), ("path", &path.to_string_lossy())])?
      }
      Net(host) => lua.create_table_from([("name", "net"), ("host", &host.to_string())])?,
    };
    Ok(mlua::Value::Table(table))
  }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Host {
  host: String,
  port: Option<NonZeroU16>,
}

impl Host {
  pub fn new(mut host: String) -> Result<Host, ParseIntError> {
    let port = host
      .rfind(':')
      .map(|pos| host.split_off(pos)[1..].parse::<NonZeroU16>())
      .transpose()?;
    Ok(Host { host, port })
  }
}

impl Display for Host {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    f.write_str(&self.host)?;
    if let Some(port) = self.port {
      f.write_char(':')?;
      f.write_str(&port.to_string())?;
    }
    Ok(())
  }
}

#[derive(Debug)]
pub struct PermissionSet(HashSet<Permission>);

impl PermissionSet {
  pub fn new() -> Self {
    Self(HashSet::new())
  }

  pub fn insert(&mut self, p: Permission) {
    // 1. remove all subsets
    // 2. if there is a superset, don't insert
    // 3. if not, insert
    let mut insert_flag = true;

    self.0.retain(|perm| {
      if perm.is_superset(&p) {
        insert_flag = false;
      }
      perm.is_subset(&p)
    });

    if insert_flag {
      self.0.insert(p);
    }
  }

  pub fn remove(&mut self, p: &Permission) {
    // remove all subsets
    self.0.retain(|x| !x.is_subset(&p));
  }

  pub fn check(&self, p: &Permission) -> bool {
    self.0.iter().find(|x| x.is_superset(p)).is_some()
  }
}
