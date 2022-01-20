use mlua::{ExternalError, FromLua, Lua};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Key {
  Boolean(bool),
  Integer(i64),
  Number(f64),
  String(#[serde(with = "serde_bytes")] Box<[u8]>),
}

impl<'lua> FromLua<'lua> for Key {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    use mlua::Value::*;
    let result = match lua_value {
      Boolean(x) => Self::Boolean(x),
      Integer(x) => Self::Integer(x),
      Number(x) => Self::Number(x),
      String(x) => Self::String(x.as_bytes().into()),
      _ => {
        return Err(
          format!(
            "invalid key: expected boolean, number or string, got {}",
            lua_value.type_name()
          )
          .to_lua_err(),
        )
      }
    };
    Ok(result)
  }
}

impl Hash for Key {
  fn hash<H: Hasher>(&self, state: &mut H) {
    use Key::*;
    match self {
      Boolean(x) => state.write_u8(*x as _),
      Integer(x) => state.write_i64(*x),
      Number(x) => state.write(&x.to_be_bytes()),
      String(x) => state.write(&x),
    }
  }
}

impl Eq for Key {}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
  Nil,
  Boolean(bool),
  Integer(i64),
  Number(f64),
  Array(Vec<Value>),
  String(#[serde(with = "serde_bytes")] Box<[u8]>),
  Map(HashMap<Key, Value>),
}
