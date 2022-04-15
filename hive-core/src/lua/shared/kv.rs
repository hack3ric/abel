use super::SharedTable;
use mlua::{ExternalError, ExternalResult, FromLua, Lua, ToLua};
use serde::{Serialize, Serializer};
use smallvec::SmallVec;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum SharedTableValue {
  Nil,
  Boolean(bool),
  Integer(i64),
  Number(f64),
  String(#[serde(serialize_with = "serialize_slice_as_str")] SmallVec<[u8; 32]>),
  Table(SharedTable),
}

fn serialize_slice_as_str<S: Serializer>(slice: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
  if let Ok(x) = std::str::from_utf8(slice) {
    serializer.serialize_str(x)
  } else {
    serializer.serialize_bytes(slice)
  }
}

impl<'lua> FromLua<'lua> for SharedTableValue {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
    use mlua::Value::*;
    let result = match lua_value {
      Nil => Self::Nil,
      Boolean(x) => Self::Boolean(x),
      Integer(x) => Self::Integer(x),
      Number(x) => Self::Number(x),
      String(x) => Self::String(x.as_bytes().into()),
      Table(x) => Self::Table(self::SharedTable::from_lua_table(lua, x)?),
      UserData(x) => {
        if let Ok(x) = x.borrow::<self::SharedTable>() {
          Self::Table(x.clone())
        } else {
          return Err("invalid table value".to_lua_err());
        }
      }
      _ => return Err("invalid table value".to_lua_err()),
    };
    Ok(result)
  }
}

impl<'a, 'lua> ToLua<'lua> for &'a SharedTableValue {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    use SharedTableValue::*;
    let result = match self {
      Nil => mlua::Value::Nil,
      Boolean(x) => mlua::Value::Boolean(*x),
      Integer(x) => mlua::Value::Integer(*x),
      Number(x) => mlua::Value::Number(*x),
      String(x) => mlua::Value::String(lua.create_string(x)?),
      Table(x) => mlua::Value::UserData(lua.create_ser_userdata(x.clone())?),
    };
    Ok(result)
  }
}

impl<'lua> ToLua<'lua> for SharedTableValue {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    use SharedTableValue::*;
    let result = match self {
      Nil => mlua::Value::Nil,
      Boolean(x) => mlua::Value::Boolean(x),
      Integer(x) => mlua::Value::Integer(x),
      Number(x) => mlua::Value::Number(x),
      String(x) => mlua::Value::String(lua.create_string(&x)?),
      Table(x) => mlua::Value::UserData(lua.create_ser_userdata(x)?),
    };
    Ok(result)
  }
}

impl PartialEq for SharedTableValue {
  fn eq(&self, other: &Self) -> bool {
    use SharedTableValue::*;
    match (self, other) {
      (Nil, Nil) => true,
      (Nil, _) => false,

      (Boolean(x), Boolean(y)) => x == y,
      (Boolean(_), _) => false,

      (Integer(x), Integer(y)) => x == y,
      (Integer(x), Number(y)) => *x as f64 == *y,
      (Integer(_), _) => false,

      (Number(x), Number(y)) => x == y,
      (Number(x), Integer(y)) => *x == *y as f64,
      (Number(_), _) => false,

      (String(x), String(y)) => x == y,
      (String(_), _) => false,

      (Table(x), Table(y)) => Arc::ptr_eq(&x.0, &y.0),
      (Table(_), _) => false,
    }
  }
}

#[derive(Clone, Serialize)]
pub struct SharedTableKey(pub(super) SharedTableValue);

#[derive(Debug, thiserror::Error)]
#[error("invalid key")]
pub struct InvalidKey(());

impl SharedTableKey {
  pub fn from_value(value: SharedTableValue) -> Result<Self, InvalidKey> {
    use SharedTableValue::*;
    match value {
      Nil => Err(InvalidKey(())),
      Table(_) => Err(InvalidKey(())),
      Number(x) if x.is_nan() => Err(InvalidKey(())),
      Number(x) => {
        let i = x as i64;
        if i as f64 == x {
          Ok(Self(Integer(i)))
        } else {
          Ok(Self(value))
        }
      }
      _ => Ok(Self(value)),
    }
  }

  pub fn to_i64(&self) -> Option<i64> {
    if let SharedTableValue::Integer(i) = self.0 {
      Some(i)
    } else {
      None
    }
  }
}

impl Hash for SharedTableKey {
  fn hash<H: Hasher>(&self, state: &mut H) {
    use SharedTableValue::*;

    fn canonical_float_bytes(f: f64) -> [u8; 8] {
      assert!(!f.is_nan());
      if f == 0.0 {
        0.0f64.to_ne_bytes()
      } else {
        f.to_ne_bytes()
      }
    }

    match &self.0 {
      Boolean(x) => (0u8, x).hash(state),
      Integer(x) => (1u8, x).hash(state),
      Number(x) => (2u8, canonical_float_bytes(*x)).hash(state),
      String(x) => (3u8, x).hash(state),
      Nil => unreachable!(),
      Table(_) => unreachable!(),
    }
  }
}

impl PartialEq for SharedTableKey {
  fn eq(&self, other: &Self) -> bool {
    self.0 == other.0
  }
}

impl Eq for SharedTableKey {}

impl<'lua> FromLua<'lua> for SharedTableKey {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
    Self::from_value(SharedTableValue::from_lua(lua_value, lua)?).to_lua_err()
  }
}

impl<'a, 'lua> ToLua<'lua> for &'a SharedTableKey {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    (&self.0).to_lua(lua)
  }
}

impl<'a, 'lua> ToLua<'lua> for SharedTableKey {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    self.0.to_lua(lua)
  }
}
