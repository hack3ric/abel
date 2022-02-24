use mlua::{ExternalError, ExternalResult, FromLua, Function, Lua, ToLua, UserData};
use parking_lot::lock_api::ArcRwLockWriteGuard;
use parking_lot::{MappedRwLockReadGuard, RawRwLock, RwLock, RwLockReadGuard};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone)]
pub struct Table(Arc<RwLock<(BTreeMap<i64, Value>, HashMap<Key, Value>)>>);

// impl Serialize

impl Table {
  pub fn new() -> Self {
    Self(Default::default())
  }

  pub fn from_lua_table(table: mlua::Table) -> mlua::Result<Self> {
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
    Ok(Self(Arc::new(RwLock::new((int, other)))))
  }

  fn get(&self, key: Key) -> MappedRwLockReadGuard<'_, Value> {
    const CONST_NIL: Value = Value::Nil;
    let lock = self.0.read();

    RwLockReadGuard::map(lock, |(x, y)| {
      (key.to_i64())
        .map(|i| x.get(&i))
        .unwrap_or_else(|| y.get(&key))
        .unwrap_or(&CONST_NIL)
    })
  }

  fn set(&self, key: Key, value: Value) -> Value {
    let mut lock = self.0.write();
    if let Some(i) = key.to_i64() {
      lock.0.insert(i, value)
    } else {
      lock.1.insert(key, value)
    }
    .unwrap_or(Value::Nil)
  }

  fn shallow_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    let lock = self.0.read();
    let int_iter = (lock.0)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let other_iter = (lock.1)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let t = lua.create_table_with_capacity(lock.0.len() as _, lock.1.len() as _)?;
    for kv in int_iter.chain(other_iter) {
      let (k, v) = kv?;
      t.raw_set(k, v)?;
    }
    Ok(t)
  }

  fn deep_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    Self::_deep_dump(&self.0, lua, &mut HashMap::new())
  }

  fn _deep_dump<'lua>(
    this: &Arc<RwLock<(BTreeMap<i64, Value>, HashMap<Key, Value>)>>,
    lua: &'lua Lua,
    tables: &mut HashMap<usize, mlua::Table<'lua>>,
  ) -> mlua::Result<mlua::Table<'lua>> {
    // preserve recursive structure
    if let Some(table) = tables.get(&(Arc::as_ptr(this) as _)) {
      Ok(table.clone())
    } else {
      let lock = this.read();
      let int_iter = (lock.0)
        .iter()
        .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
      let other_iter = (lock.1)
        .iter()
        .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
      let table = lua.create_table_with_capacity(lock.0.len() as _, lock.1.len() as _)?;
      tables.insert(Arc::as_ptr(this) as _, table.clone());
      for kv in int_iter.chain(other_iter) {
        let (k, v) = kv?;
        if let Value::Table(Table(table_repr)) = v {
          let sub_table = Self::_deep_dump(table_repr, lua, tables)?;
          table.raw_set(k, sub_table)?;
        } else {
          table.raw_set(k, v)?;
        }
      }
      Ok(table)
    }
  }
}

impl UserData for Table {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_method("__index", |lua, this, key: Key| {
      let result = this.get(key);
      (&*result).to_lua(lua)
    });

    methods.add_meta_method("__newindex", |_lua, this, (key, value): (Key, Value)| {
      this.set(key, value);
      Ok(())
    });

    methods.add_meta_method("__len", |_lua, this, ()| Ok(len(&this.0.read())));

    methods.add_meta_method("__pairs", |lua, this, ()| {
      let next: mlua::Value = lua.globals().raw_get("next")?;
      let table = this.shallow_dump(lua)?;
      Ok((next, table, mlua::Value::Nil))
    });
  }
}

struct TableScope(ArcRwLockWriteGuard<RawRwLock, (BTreeMap<i64, Value>, HashMap<Key, Value>)>);

impl TableScope {
  fn new(x: Arc<RwLock<(BTreeMap<i64, Value>, HashMap<Key, Value>)>>) -> Self {
    Self(x.write_arc())
  }

  fn get(&self, key: Key) -> &Value {
    const CONST_NIL: Value = Value::Nil;

    (key.to_i64())
      .map(|i| self.0 .0.get(&i))
      .unwrap_or_else(|| self.0 .1.get(&key))
      .unwrap_or(&CONST_NIL)
  }

  fn set(&mut self, key: Key, value: Value) -> Value {
    if let Some(i) = key.to_i64() {
      self.0 .0.insert(i, value)
    } else {
      self.0 .1.insert(key, value)
    }
    .unwrap_or(Value::Nil)
  }

  fn shallow_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    let int_iter = (self.0 .0)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let other_iter = (self.0 .1)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let t = lua.create_table_with_capacity(self.0 .0.len() as _, self.0 .1.len() as _)?;
    for kv in int_iter.chain(other_iter) {
      let (k, v) = kv?;
      t.raw_set(k, v)?;
    }
    Ok(t)
  }

  fn deep_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    let mut tables = HashMap::new();
    let int_iter = (self.0 .0)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let other_iter = (self.0 .1)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let table = lua.create_table_with_capacity(self.0 .0.len() as _, self.0 .1.len() as _)?;
    tables.insert(
      Arc::as_ptr(ArcRwLockWriteGuard::rwlock(&self.0)) as _,
      table.clone(),
    );
    for kv in int_iter.chain(other_iter) {
      let (k, v) = kv?;
      if let Value::Table(Table(table_repr)) = v {
        let sub_table = Table::_deep_dump(table_repr, lua, &mut tables)?;
        table.raw_set(k, sub_table)?;
      } else {
        table.raw_set(k, v)?;
      }
    }
    Ok(table)
  }
}

impl UserData for TableScope {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_method("__index", |lua, this, key: Key| {
      let result = this.get(key);
      (&*result).to_lua(lua)
    });

    methods.add_meta_method_mut("__newindex", |_lua, this, (key, value): (Key, Value)| {
      this.set(key, value);
      Ok(())
    });

    methods.add_meta_method("__len", |_lua, this, ()| Ok(len(&this.0)));

    methods.add_meta_method("__pairs", |lua, this, ()| {
      let next: mlua::Value = lua.globals().raw_get("next")?;
      let table = this.shallow_dump(lua)?;
      Ok((next, table, mlua::Value::Nil))
    });
  }
}

pub fn create_fn_table_dump(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, table: mlua::Value| match table {
    mlua::Value::Table(table) => Ok(table),
    mlua::Value::UserData(x) => {
      if let Ok(x) = x.borrow::<Table>() {
        x.deep_dump(lua)
      } else if let Ok(x) = x.borrow::<TableScope>() {
        x.deep_dump(lua)
      } else {
        Err("failed to borrow userdata as shared table".to_lua_err())
      }
    }
    _ => Err("expected table or shared table".to_lua_err()),
  })
}

pub fn create_fn_table_scope(lua: &Lua) -> mlua::Result<Function> {
  lua.create_async_function(|lua, (table, f): (mlua::Value, Function)| async move {
    match table {
      mlua::Value::Table(table) => f.call_async(table).await,
      mlua::Value::UserData(x) => {
        if let Ok(x) = x.borrow::<Table>() {
          let x = lua.create_userdata(TableScope::new(x.0.clone()))?;
          let result = f.call_async::<_, mlua::Value>(x.clone()).await;
          x.take::<TableScope>()?;
          return result;
        }
        if x.borrow::<TableScope>().is_ok() {
          f.call_async::<_, mlua::Value>(x).await
        } else {
          Err("failed to borrow userdata as shared table".to_lua_err())
        }
      }
      _ => Err("expected table or shared table".to_lua_err()),
    }
  })
}

pub fn create_fn_table_insert_shared_3(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(
    |lua, (table, pos, value): (mlua::AnyUserData, i64, mlua::Value)| {
      if pos < 1 {
        return Err("out of bounds".to_lua_err());
      }
      if let Ok(table) = table.borrow::<Table>() {
        let mut lock = table.0.write();
        if pos > len(&lock) + 1 {
          return Err("out of bounds".to_lua_err());
        }
        let right = lock.0.split_off(&pos);
        let iter = right.into_iter().map(|(i, v)| (i + 1, v));
        lock.0.insert(pos, Value::from_lua(value, lua)?);
        lock.0.extend(iter);
      } else if let Ok(mut table) = table.borrow_mut::<TableScope>() {
        if pos > len(&table.0) + 1 {
          return Err("out of bounds".to_lua_err());
        }
        let right = table.0 .0.split_off(&pos);
        let iter = right.into_iter().map(|(i, v)| (i + 1, v));
        table.0 .0.insert(pos, Value::from_lua(value, lua)?);
        table.0 .0.extend(iter);
      } else {
        return Err("failed to borrow userdata as shared table".to_lua_err());
      }
      Ok(())
    },
  )
}

fn len(x: &(BTreeMap<i64, Value>, HashMap<Key, Value>)) -> i64 {
  x.0.iter().last().map(|x| (*x.0).max(0)).unwrap_or(0)
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
      Table(x) => Self::Table(self::Table::from_lua_table(x)?),
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
