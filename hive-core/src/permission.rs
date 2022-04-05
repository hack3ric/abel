use crate::path::normalize_path;
use crate::{ErrorKind, Result};
use mlua::{FromLua, Lua, LuaSerdeExt, ToLua};
use nonzero_ext::nonzero;
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::{Display, Formatter, Write};
use std::num::NonZeroU16;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Permission<'a> {
  #[serde(rename = "read")]
  Read { path: Cow<'a, Path> },
  #[serde(rename = "write")]
  Write { path: Cow<'a, Path> },
  #[serde(rename = "net")]
  Net {
    host: Cow<'a, str>,
    #[serde(default = "default_port")]
    port: NonZeroU16,
  },
  #[serde(rename = "env")]
  Env { name: Cow<'a, str> },
}

const fn default_port() -> NonZeroU16 {
  nonzero!(443u16)
}

impl<'a> Permission<'a> {
  pub const fn name(&self) -> &'static str {
    use Permission::*;
    match self {
      Read { .. } => "read",
      Write { .. } => "write",
      Net { .. } => "net",
      Env { .. } => "env",
    }
  }

  pub fn is_subset(&self, other: &Self) -> bool {
    use Permission::*;
    match (self, other) {
      (Read { path: p1 }, Read { path: p2 }) | (Write { path: p1 }, Write { path: p2 }) => {
        p1.starts_with(p2)
      }
      _ => self == other,
    }
  }

  pub fn is_superset(&self, other: &Self) -> bool {
    use Permission::*;
    match (self, other) {
      (Read { path: p1 }, Read { path: p2 }) | (Write { path: p1 }, Write { path: p2 }) => {
        p2.starts_with(p1)
      }
      _ => self == other,
    }
  }

  pub fn into_owned(self) -> Permission<'static> {
    use Permission::*;
    match self {
      Read { path } => Read {
        path: Cow::Owned(path.into_owned()),
      },
      Write { path } => Write {
        path: Cow::Owned(path.into_owned()),
      },
      Net { host, port } => Net {
        host: Cow::Owned(host.into_owned()),
        port,
      },
      Env { name } => Env {
        name: Cow::Owned(name.into_owned()),
      },
    }
  }

  pub fn normalize(&mut self) {
    use Permission::*;
    match self {
      Read { path } | Write { path } => *path = Cow::Owned(normalize_path(&path)),
      _ => {}
    }
  }
}

impl<'lua> FromLua<'lua> for Permission<'lua> {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
    lua.from_value(lua_value)
  }
}

impl<'lua, 'a> ToLua<'lua> for Permission<'a> {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    lua.to_value(&self)
  }
}

impl<'a> Display for Permission<'a> {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    use Permission::*;
    f.write_str(self.name())?;
    f.write_str("::")?;
    match self {
      Read { path } | Write { path } => {
        // XXX: assuming it's all UTF-8
        f.write_str(&path.to_string_lossy())?;
      }
      Net { host, port } => {
        f.write_str(host)?;
        if u16::from(*port) != 443 {
          f.write_char(':')?;
          f.write_str(&port.to_string())?;
        }
      }
      Env { name } => f.write_str(name)?,
    }
    Ok(())
  }
}

#[derive(Debug, Serialize, Default)]
pub struct PermissionSet(HashSet<Permission<'static>>);

impl PermissionSet {
  pub fn new() -> Self {
    Default::default()
  }

  pub fn insert(&mut self, p: Permission<'_>) {
    self.insert_owned(p.into_owned())
  }

  pub fn insert_owned(&mut self, p: Permission<'static>) {
    // 1. remove all subsets
    // 2. if there is a superset, don't insert
    // 3. if not, insert
    let mut insert_flag = true;

    self.0.retain(|perm| {
      if perm.is_superset(&p) {
        insert_flag = false;
      }
      !perm.is_subset(&p)
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
      Err(
        ErrorKind::PermissionNotGranted {
          permission: p.clone().into_owned(),
        }
        .into(),
      )
    }
  }
}

impl<'de> Deserialize<'de> for PermissionSet {
  fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
    let v = Vec::<Permission>::deserialize(de)?;
    let mut s = Self::new();
    for i in v.into_iter() {
      s.insert(i);
    }
    Ok(s)
  }
}
