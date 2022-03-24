use super::BadArgument;
use dashmap::DashMap;
use mlua::{AnyUserData, ExternalError, ExternalResult, FromLua, Function, Lua, ToLua, UserData};
use once_cell::sync::Lazy;
use parking_lot::lock_api::ArcRwLockWriteGuard;
use parking_lot::{MappedRwLockReadGuard, RawRwLock, RwLock, RwLockReadGuard};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use smallvec::SmallVec;

type ContextStore = Arc<DashMap<Box<str>, Table>>;

static CONTEXT_STORE: Lazy<ContextStore> = Lazy::new(|| Arc::new(DashMap::new()));

pub fn create_module_context(lua: &Lua, service_name: Box<str>) -> mlua::Result<AnyUserData> {
  let context = CONTEXT_STORE
    .entry(service_name)
    .or_insert(Table::new())
    .clone();
  lua.create_ser_userdata(context)
}

pub fn remove_service_contexts(service_name: &str) {
  CONTEXT_STORE.retain(|k, _| k.as_ref() != service_name);
}

#[derive(Clone, Default)]
pub struct Table(Arc<RwLock<TableRepr>>);

impl Table {
  pub fn new() -> Self {
    Default::default()
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
    Ok(Self(Arc::new(RwLock::new(TableRepr(int, other)))))
  }

  fn get(&self, key: Key) -> MappedRwLockReadGuard<'_, Value> {
    RwLockReadGuard::map(self.0.read(), |x| x.get(key))
  }

  fn set(&self, key: Key, value: Value) -> Value {
    self.0.write().set(key, value)
  }

  fn shallow_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    self.0.read().shallow_dump(lua)
  }

  fn deep_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    self._deep_dump(lua, &mut HashMap::new())
  }

  fn _deep_dump<'lua>(
    &self,
    lua: &'lua Lua,
    tables: &mut HashMap<usize, mlua::Table<'lua>>,
  ) -> mlua::Result<mlua::Table<'lua>> {
    // preserve recursive structure
    if let Some(table) = tables.get(&(Arc::as_ptr(&self.0) as _)) {
      Ok(table.clone())
    } else {
      (self.0.read())._deep_dump(lua, Arc::as_ptr(&self.0) as _, tables)
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

impl Serialize for Table {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.read().serialize(serializer)
  }
}

struct TableScope(ArcRwLockWriteGuard<RawRwLock, TableRepr>);

impl TableScope {
  fn new(x: Arc<RwLock<TableRepr>>) -> Self {
    Self(x.write_arc())
  }

  fn deep_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    self._deep_dump(
      lua,
      Arc::as_ptr(ArcRwLockWriteGuard::rwlock(&self.0)) as _,
      &mut HashMap::new(),
    )
  }
}

impl Deref for TableScope {
  type Target = TableRepr;

  fn deref(&self) -> &TableRepr {
    &self.0
  }
}

impl DerefMut for TableScope {
  fn deref_mut(&mut self) -> &mut TableRepr {
    &mut self.0
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

#[derive(Default)]
struct TableRepr(BTreeMap<i64, Value>, HashMap<Key, Value>);

impl TableRepr {
  fn get(&self, key: Key) -> &Value {
    const CONST_NIL: Value = Value::Nil;

    (key.to_i64())
      .map(|i| self.0.get(&i))
      .unwrap_or_else(|| self.1.get(&key))
      .unwrap_or(&CONST_NIL)
  }

  fn set(&mut self, key: Key, value: Value) -> Value {
    if let Some(i) = key.to_i64() {
      self.0.insert(i, value)
    } else {
      self.1.insert(key, value)
    }
    .unwrap_or(Value::Nil)
  }

  fn shallow_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<mlua::Table<'lua>> {
    let int_iter = (self.0)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let other_iter = (self.1)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let t = lua.create_table_with_capacity(self.0.len() as _, self.1.len() as _)?;
    for kv in int_iter.chain(other_iter) {
      let (k, v) = kv?;
      t.raw_set(k, v)?;
    }
    Ok(t)
  }

  fn _deep_dump<'lua>(
    &self,
    lua: &'lua Lua,
    ptr: usize,
    tables: &mut HashMap<usize, mlua::Table<'lua>>,
  ) -> mlua::Result<mlua::Table<'lua>> {
    let int_iter = (self.0)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let other_iter = (self.1)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let table = lua.create_table_with_capacity(self.0.len() as _, self.1.len() as _)?;
    tables.insert(ptr, table.clone());
    for kv in int_iter.chain(other_iter) {
      let (k, v) = kv?;
      if let Value::Table(x @ Table(_)) = v {
        let sub_table = Table::_deep_dump(x, lua, tables)?;
        table.raw_set(k, sub_table)?;
      } else {
        table.raw_set(k, v)?;
      }
    }
    Ok(table)
  }
}

impl Serialize for TableRepr {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    if self.0.contains_key(&1) {
      let mut seq = serializer.serialize_seq(None)?;
      let mut prev = 0;
      for (&i, v) in self.0.iter() {
        if i < 1 {
          continue;
        }
        if i - prev != 1 {
          break;
        }
        seq.serialize_element(v)?;
        prev = i;
      }
      seq.end()
    } else {
      self.1.serialize(serializer)
    }
  }
}

fn userdata_not_shared_table(fn_name: &'static str, pos: u8) -> mlua::Error {
  BadArgument::new(fn_name, pos, "failed to borrow userdata as shared table").into()
}

fn expected_table(fn_name: &'static str, pos: u8, found: &str) -> mlua::Error {
  BadArgument::new(
    fn_name,
    pos,
    format!("expected table or shared table, found {found}"),
  )
  .into()
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
        Err(userdata_not_shared_table("dump", 1))
      }
    }
    _ => Err(expected_table("dump", 1, table.type_name())),
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
          Err(userdata_not_shared_table("scope", 1))
        }
      }
      _ => Err(expected_table("scope", 1, table.type_name())),
    }
  })
}

fn out_of_bounds(fn_name: &'static str, pos: u8) -> mlua::Error {
  BadArgument::new(fn_name, pos, "out of bounds").into()
}

pub fn create_fn_table_insert_shared_3(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(
    |lua, (table, pos, value): (mlua::AnyUserData, i64, mlua::Value)| {
      if pos < 1 {
        return Err(out_of_bounds("insert", 2));
      }
      if let Ok(table) = table.borrow::<Table>() {
        let mut lock = table.0.write();
        if pos > len(&lock) + 1 {
          return Err(out_of_bounds("insert", 2));
        }
        let right = lock.0.split_off(&pos);
        let iter = right.into_iter().map(|(i, v)| (i + 1, v));
        lock.0.insert(pos, Value::from_lua(value, lua)?);
        lock.0.extend(iter);
      } else if let Ok(mut table) = table.borrow_mut::<TableScope>() {
        if pos > len(&table.0) + 1 {
          return Err(out_of_bounds("insert", 2));
        }
        let right = table.0 .0.split_off(&pos);
        let iter = right.into_iter().map(|(i, v)| (i + 1, v));
        table.0 .0.insert(pos, Value::from_lua(value, lua)?);
        table.0 .0.extend(iter);
      } else {
        return Err(userdata_not_shared_table("insert", 1));
      }
      Ok(())
    },
  )
}

fn len(x: &TableRepr) -> i64 {
  x.0.iter().last().map(|x| (*x.0).max(0)).unwrap_or(0)
}

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum Value {
  Nil,
  Boolean(bool),
  Integer(i64),
  Number(f64),
  String(#[serde(serialize_with = "serialize_slice_as_str")] SmallVec<[u8; 32]>),
  Table(Table),
}

fn serialize_slice_as_str<S: Serializer>(slice: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
  if let Ok(x) = std::str::from_utf8(slice) {
    serializer.serialize_str(x)
  } else {
    serializer.serialize_bytes(slice)
  }
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
      Table(x) => mlua::Value::UserData(lua.create_ser_userdata(x.clone())?),
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
      Table(x) => mlua::Value::UserData(lua.create_ser_userdata(x)?),
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

#[derive(Clone, Serialize)]
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
