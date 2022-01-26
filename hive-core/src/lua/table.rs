use mlua::{ExternalError, ExternalResult, FromLua, Lua, ToLua, UserData};
use parking_lot::{MappedRwLockReadGuard, RwLock, RwLockReadGuard};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

// TODO: scope Table

#[derive(Clone)]
pub struct Table(Arc<TableRepr>);

impl UserData for Table {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_method("__index", |lua, this, key: Key| {
      (&*this.0.get(key)).to_lua(lua)
    });
    methods.add_method("__newindex", |_lua, this, (key, value): (Key, Value)| {
      this.0.set(key, value);
      Ok(())
    });
  }
}

struct TableRepr {
  inner: RwLock<(BTreeMap<i64, Value>, HashMap<Key, Value>)>,
}

impl TableRepr {
  fn from_table(table: mlua::Table) -> mlua::Result<Self> {
    let mut int = BTreeMap::new();
    let mut other = HashMap::new();
    for kv in table.pairs::<Key, Value>() {
      let (k, v) = kv?;
      if let Some(i) = k.to_i64() {
        int.insert(i, v);
      } else {
        other.insert(k, v);
      }
    }
    Ok(Self {
      inner: RwLock::new((int, other)),
    })
  }

  fn get(&self, key: Key) -> MappedRwLockReadGuard<'_, Value> {
    const CONST_NIL: Value = Value::Nil;
    let lock = self.inner.read();
    RwLockReadGuard::map(lock, |(x, y)| {
      key
        .to_i64()
        .map(|i| x.get(&i))
        .unwrap_or_else(|| y.get(&key))
        .unwrap_or(&CONST_NIL)
    })
  }

  fn set(&self, key: Key, value: Value) -> Value {
    let mut lock = self.inner.write();
    if let Some(i) = key.to_i64() {
      lock.0.insert(i, value)
    } else {
      lock.1.insert(key, value)
    }
    .unwrap_or(Value::Nil)
  }
}

#[derive(Clone)]
pub enum Value {
  Nil,
  Boolean(bool),
  Integer(i64),
  Number(f64),
  String(Vec<u8>),
  Table(Table),
}

impl<'lua> FromLua<'lua> for Value {
  fn from_lua(lua_value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    use mlua::Value::*;
    let result = match lua_value {
      Nil => Self::Nil,
      Boolean(x) => Self::Boolean(x),
      Integer(x) => Self::Integer(x),
      Number(x) => Self::Number(x),
      String(x) => Self::String(x.as_bytes().into()),
      Table(x) => Self::Table(self::Table(Arc::new(TableRepr::from_table(x)?))),
      UserData(x) => {
        if let Ok(x) = x.borrow::<self::Table>() {
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

impl<'a, 'lua> ToLua<'lua> for &'a Value {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    use Value::*;
    let result = match self {
      Nil => mlua::Value::Nil,
      Boolean(x) => mlua::Value::Boolean(*x),
      Integer(x) => mlua::Value::Integer(*x),
      Number(x) => mlua::Value::Number(*x),
      String(x) => mlua::Value::String(lua.create_string(x)?),
      Table(x) => x.clone().to_lua(lua)?,
    };
    Ok(result)
  }
}

impl<'lua> ToLua<'lua> for Value {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    use Value::*;
    let result = match self {
      Nil => mlua::Value::Nil,
      Boolean(x) => mlua::Value::Boolean(x),
      Integer(x) => mlua::Value::Integer(x),
      Number(x) => mlua::Value::Number(x),
      String(x) => mlua::Value::String(lua.create_string(&x)?),
      Table(x) => x.to_lua(lua)?,
    };
    Ok(result)
  }
}

impl PartialEq for Value {
  fn eq(&self, other: &Self) -> bool {
    use Value::*;
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

#[derive(Clone)]
pub struct Key(Value);

#[derive(Debug, thiserror::Error)]
#[error("invalid key")]
pub struct InvalidKey(());

impl Key {
  pub fn from_value(value: Value) -> Result<Self, InvalidKey> {
    use Value::*;
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
    if let Value::Integer(i) = self.0 {
      Some(i)
    } else {
      None
    }
  }
}

impl Hash for Key {
  fn hash<H: Hasher>(&self, state: &mut H) {
    use Value::*;

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

impl PartialEq for Key {
  fn eq(&self, other: &Self) -> bool {
    self.0 == other.0
  }
}

impl Eq for Key {}

impl<'lua> FromLua<'lua> for Key {
  fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua Lua) -> mlua::Result<Self> {
    Self::from_value(Value::from_lua(lua_value, lua)?).to_lua_err()
  }
}

impl<'a, 'lua> ToLua<'lua> for &'a Key {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    (&self.0).to_lua(lua)
  }
}

impl<'a, 'lua> ToLua<'lua> for Key {
  fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
    self.0.to_lua(lua)
  }
}
