use crate::path::normalize_path;
use crate::{ErrorKind, Result};
use mlua::{ExternalResult, FromLua, Lua, ToLua};
use nonzero_ext::nonzero;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::{Display, Formatter, Write};
use std::num::{NonZeroU16, ParseIntError};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Permission<'a> {
  Read(Cow<'a, Path>),
  Write(Cow<'a, Path>),
  Net(Cow<'a, str>, NonZeroU16),
  Env(Cow<'a, str>),
}

impl<'a> Permission<'a> {
  pub fn name(&self) -> &'static str {
    use Permission::*;
    match self {
      Read(_) => "read",
      Write(_) => "write",
      Net(_, _) => "net",
      Env(_) => "env",
    }
  }

  pub fn is_subset(&self, other: &Self) -> bool {
    use Permission::*;
    match (self, other) {
      (Read(p1), Read(p2)) | (Write(p1), Write(p2)) => p1.starts_with(p2),
      _ => self == other,
    }
  }

  pub fn is_superset(&self, other: &Self) -> bool {
    use Permission::*;
    match (self, other) {
      (Read(p1), Read(p2)) | (Write(p1), Write(p2)) => p2.starts_with(p1),
      _ => self == other,
    }
  }

  pub fn into_owned(self) -> Permission<'static> {
    use Permission::*;
    match self {
      Read(x) => Read(Cow::Owned(x.into_owned())),
      Write(x) => Write(Cow::Owned(x.into_owned())),
      Net(x, p) => Net(Cow::Owned(x.into_owned()), p),
      Env(x) => Env(Cow::Owned(x.into_owned())),
    }
  }

  pub fn normalize(&mut self) {
    use Permission::*;
    match self {
      Read(x) | Write(x) => *x = Cow::Owned(normalize_path(&x)),
      _ => {}
    }
  }

  pub fn parse(s: &'a str) -> Result<Self> {
    let result = Self::_parse(s).map_err(|error| ErrorKind::InvalidPermission {
      string: s.into(),
      reason: error.as_ref().into(),
    })?;
    Ok(result)
  }

  fn _parse(s: &'a str) -> Result<Self, Cow<'static, str>> {
    use Permission::*;

    let (scheme, content) = s.split_once("::").ok_or("permission scheme not given")?;
    let result = match scheme {
      "read" => Read(Path::new(content).into()),
      "write" => Write(Path::new(content).into()),
      "net" => {
        let (host, port) = content
          .rsplit_once(':')
          .map(|(x, p)| Ok::<_, ParseIntError>((x, p.parse::<NonZeroU16>()?)))
          .transpose()
          .map_err(|e| format!("failed to parse port: {e}"))?
          .unwrap_or((content, nonzero!(443u16)));
        Net(host.into(), port)
      }
      "env" => Env(content.into()),
      _ => return Err(format!("unknown permission scheme: {s}").into()),
    };

    Ok(result)
  }
}

impl<'lua> FromLua<'lua> for Permission<'static> {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
    let s = lua.unpack::<mlua::String>(lua_value)?;
    let s = std::str::from_utf8(s.as_bytes()).to_lua_err()?;
    Permission::_parse(s)
      .map(Permission::into_owned)
      .to_lua_err()
  }
}

impl<'lua, 'a> ToLua<'lua> for Permission<'a> {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    lua.pack(self.to_string())
  }
}

impl<'a> Display for Permission<'a> {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    use Permission::*;
    f.write_str(self.name())?;
    f.write_str("::")?;
    match self {
      Read(x) | Write(x) => {
        // XXX: assuming it's all UTF-8
        f.write_str(&x.to_string_lossy())?;
      }
      Net(x, p) => {
        f.write_str(x)?;
        if u16::from(*p) != 443 {
          f.write_char(':')?;
          f.write_str(&p.to_string())?;
        }
      }
      Env(x) => f.write_str(x)?,
    }
    Ok(())
  }
}

impl<'a> Serialize for Permission<'a> {
  fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
    self.to_string().serialize(ser)
  }
}

impl<'de: 'a, 'a> Deserialize<'de> for Permission<'a> {
  fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
    Self::_parse(<&str>::deserialize(de)?).map_err(serde::de::Error::custom)
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

#[cfg(test)]
mod tests {
  use super::*;
  use test_case::test_case;

  fn p(s: &str) -> Permission {
    Permission::parse(s).unwrap()
  }

  #[test_case(p("read::/foo/bar/baz"), p("read::/foo/bar") => (true, false); "subpath")]
  #[test_case(p("read::/foo/bar"), p("read::/foo/bar/baz") => (false, true); "super path")]
  #[test_case(p("read::/foo/bar"), p("read::/foo/bar") => (true, true); "same path")]
  #[test_case(p("read::/foo/bar"), p("write::/foo/bar") => (false, false); "different type")]
  #[test_case(p("net::httpbin.org"), p("net::httpbin.org:443") => (true, true); "defaults to 443")]
  #[test_case(p("net::httpbin.org:80"), p("net::httpbin.org") => (false, false); "not allowing raw HTTP by default")]
  fn test_subset_superset(a: Permission, b: Permission) -> (bool, bool) {
    (a.is_subset(&b), a.is_superset(&b))
  }
}
