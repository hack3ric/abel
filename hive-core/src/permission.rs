use crate::path::normalize_path;
use crate::{ErrorKind, Result};
use mlua::{ExternalError, ExternalResult, FromLua, Lua, String as LuaString, ToLua};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
use std::fmt::{Display, Write};
use std::num::{NonZeroU16, ParseIntError};
use std::path::{Path, PathBuf};

/// A permission flag.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permission(pub(crate) PermissionInner);

/// Inner of the `Permission` type, for pattern matching.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type")]
pub enum PermissionInner {
  /// Reading a file.
  #[serde(rename = "read")]
  Read { path: PathBuf },
  /// Writing a file.
  #[serde(rename = "write")]
  Write { path: PathBuf },
  /// Connecting to specified host.
  #[serde(rename = "net")]
  Net { host: Host },
}

impl Permission {
  pub fn read(path: impl AsRef<Path>) -> Self {
    Self::read_unchecked(normalize_path(path))
  }

  pub fn read_unchecked(path: impl Into<PathBuf>) -> Self {
    Self(PermissionInner::Read { path: path.into() })
  }

  pub fn write(path: impl AsRef<Path>) -> Self {
    Self::write_unchecked(normalize_path(path))
  }

  pub fn write_unchecked(path: impl Into<PathBuf>) -> Self {
    Self(PermissionInner::Write { path: path.into() })
  }

  pub fn net_parse(host: impl Into<String>) -> Result<Self, ParseIntError> {
    Ok(Self(PermissionInner::Net {
      host: Host::new(host.into())?,
    }))
  }

  pub fn net(host: impl Into<String>, port: Option<NonZeroU16>) -> Self {
    Self(PermissionInner::Net {
      host: Host {
        host: host.into(),
        port,
      },
    })
  }

  pub fn is_subset(&self, other: &Self) -> bool {
    use PermissionInner::*;
    match (&self.0, &other.0) {
      (Read { path: p1 }, Read { path: p2 }) | (Write { path: p1 }, Write { path: p2 }) => {
        p1.starts_with(p2)
      }
      (Net { host: h1 }, Net { host: h2 }) => match (&h2.port, &h1.port) {
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
      (Read { path: p1 }, Read { path: p2 }) | (Write { path: p1 }, Write { path: p2 }) => {
        p2.starts_with(p1)
      }
      (Net { host: h1 }, Net { host: h2 }) => match (&h1.port, &h2.port) {
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
      Read { .. } => "read",
      Write { .. } => "write",
      Net { .. } => "net",
    }
  }
}

impl Display for Permission {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    use PermissionInner::*;
    f.write_str(self.name())?;
    f.write_char(':')?;
    match &self.0 {
      Read { path } | Write { path } => f.write_str(&path.to_string_lossy()),
      Net { host } => <Host as Display>::fmt(host, f),
    }
  }
}

impl<'lua> FromLua<'lua> for Permission {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    if let mlua::Value::Table(table) = lua_value {
      let name: LuaString = table.raw_get("type")?;
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
          let host: String = table.raw_get("host")?;
          Self::net_parse(host).to_lua_err()
        }
        _ => Err("invalid permission type".to_lua_err()),
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
      Read { path } => {
        lua.create_table_from([("type", "read"), ("path", &path.to_string_lossy())])?
      }
      Write { path } => {
        lua.create_table_from([("type", "write"), ("path", &path.to_string_lossy())])?
      }
      Net { host } => lua.create_table_from([("type", "net"), ("host", &host.to_string())])?,
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

impl Serialize for Host {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    self.to_string().serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for Host {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let host = String::deserialize(deserializer)?;
    Self::new(host).map_err(serde::de::Error::custom)
  }
}

#[derive(Debug, Serialize, Default)]
pub struct PermissionSet(HashSet<Permission>);

impl PermissionSet {
  pub fn new() -> Self {
    Default::default()
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
    self.0.retain(|x| !x.is_subset(p));
  }

  pub fn check_ok(&self, p: &Permission) -> bool {
    self.0.iter().any(|x| x.is_superset(p))
  }

  pub fn check(&self, p: &Permission) -> Result<()> {
    if self.check_ok(p) {
      Ok(())
    } else {
      Err(ErrorKind::PermissionNotGranted(p.clone()).into())
    }
  }
}

impl<'de> Deserialize<'de> for PermissionSet {
  // This still has a lot of optimizations to do. Maybe deserialize it into
  // `HashSet` and de-duplicate it in place?
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let vec = Vec::deserialize(deserializer)?;
    let mut result = Self(HashSet::new());
    for i in vec.into_iter() {
      result.insert(i);
    }
    Ok(result)
  }
}
