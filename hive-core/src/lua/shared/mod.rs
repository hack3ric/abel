mod kv;
mod patch;

pub use kv::{SharedTableKey, SharedTableValue};
pub use patch::apply_table_module_patch;

use dashmap::DashMap;
use mlua::{AnyUserData, Function, Lua, LuaSerdeExt, MultiValue, Table, ToLua, UserData};
use once_cell::sync::Lazy;
use parking_lot::lock_api::ArcRwLockWriteGuard;
use parking_lot::{MappedRwLockReadGuard, RawRwLock, RwLock, RwLockReadGuard};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

type SharedStore = Arc<DashMap<Box<str>, SharedTable>>;

static SHARED_STORE: Lazy<SharedStore> = Lazy::new(|| Arc::new(DashMap::new()));

pub fn create_module_shared(lua: &Lua, service_name: Box<str>) -> mlua::Result<AnyUserData> {
  let shared = SHARED_STORE
    .entry(service_name)
    .or_insert(SharedTable::new())
    .clone();
  lua.create_ser_userdata(shared)
}

pub fn remove_service_shared_stores(service_name: &str) {
  SHARED_STORE.retain(|k, _| k.as_ref() != service_name);
}

#[derive(Clone, Default)]
pub struct SharedTable(Arc<RwLock<SharedTableRepr>>);

impl SharedTable {
  pub fn new() -> Self {
    Default::default()
  }

  pub fn from_lua_table(lua: &Lua, table: Table) -> mlua::Result<Self> {
    let mut int = BTreeMap::new();
    let mut hash = HashMap::new();
    for kv in table.clone().pairs::<SharedTableKey, SharedTableValue>() {
      let (k, v) = kv?;
      if let Some(i) = k.to_i64() {
        int.insert(i, v);
      } else {
        hash.insert(k, v);
      }
    }
    let array = table
      .get_metatable()
      .map(|x| x == lua.array_metatable())
      .unwrap_or(false);
    let repr = SharedTableRepr { int, hash, array };
    Ok(Self(Arc::new(RwLock::new(repr))))
  }

  pub(crate) fn get(&self, key: SharedTableKey) -> MappedRwLockReadGuard<'_, SharedTableValue> {
    RwLockReadGuard::map(self.0.read(), |x| x.get(key))
  }

  pub(crate) fn set(&self, key: SharedTableKey, value: SharedTableValue) -> SharedTableValue {
    self.0.write().set(key, value)
  }

  fn push(&self, value: SharedTableValue) {
    let mut wl = self.0.write();
    let pos = len(&wl) + 1;
    wl.set(SharedTableKey(SharedTableValue::Integer(pos)), value);
  }

  pub fn set_array(&self, array: bool) {
    self.0.write().array = array;
  }

  #[allow(unused)]
  fn shallow_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<Table<'lua>> {
    self.0.read().shallow_dump(lua)
  }

  fn deep_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<Table<'lua>> {
    self._deep_dump(lua, &mut HashMap::new())
  }

  fn _deep_dump<'lua>(
    &self,
    lua: &'lua Lua,
    tables: &mut HashMap<usize, Table<'lua>>,
  ) -> mlua::Result<Table<'lua>> {
    // preserve recursive structure
    if let Some(table) = tables.get(&(Arc::as_ptr(&self.0) as _)) {
      Ok(table.clone())
    } else {
      (self.0.read())._deep_dump(lua, Arc::as_ptr(&self.0) as _, tables)
    }
  }
}

impl UserData for SharedTable {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_method("__index", |lua, this, key: SharedTableKey| {
      let result = this.get(key);
      lua.pack(&*result)
    });

    methods.add_meta_method(
      "__newindex",
      |_lua, this, (key, value): (SharedTableKey, SharedTableValue)| {
        this.set(key, value);
        Ok(())
      },
    );

    methods.add_meta_method("__len", |_lua, this, ()| Ok(len(&this.0.read())));

    methods.add_meta_method("__pairs", |lua, this, ()| {
      let rl = this.0.read();
      let keys = (rl.int.keys())
        .map(|i| SharedTableKey(SharedTableValue::Integer(*i)))
        .chain(rl.hash.keys().cloned())
        .map(|x| (x, true));
      let keys = lua.create_table_from(keys)?;
      let iter = lua.create_function(
        |lua, (table, keys, prev_key): (SharedTable, Table, mlua::Value)| {
          let next: Function = lua.globals().raw_get("next")?;
          let key: Option<SharedTableKey> = next.call((keys, prev_key))?;
          if let Some(key) = key {
            lua.pack_multi((key.clone(), lua.pack(&*table.get(key))?))
          } else {
            Ok(MultiValue::new())
          }
        },
      )?;
      drop(rl);
      let iter = iter.bind(this.clone())?;
      Ok((iter, keys))
    });
  }
}

impl Serialize for SharedTable {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.read().serialize(serializer)
  }
}

struct SharedTableScope(ArcRwLockWriteGuard<RawRwLock, SharedTableRepr>);

impl SharedTableScope {
  fn new(x: Arc<RwLock<SharedTableRepr>>) -> Self {
    Self(x.write_arc())
  }

  fn deep_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<Table<'lua>> {
    self.0._deep_dump(
      lua,
      Arc::as_ptr(ArcRwLockWriteGuard::rwlock(&self.0)) as _,
      &mut HashMap::new(),
    )
  }
}

impl UserData for SharedTableScope {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_method("__index", |lua, this, key: SharedTableKey| {
      let result = this.0.get(key);
      (&*result).to_lua(lua)
    });

    methods.add_meta_method_mut(
      "__newindex",
      |_lua, this, (key, value): (SharedTableKey, SharedTableValue)| {
        this.0.set(key, value);
        Ok(())
      },
    );

    methods.add_meta_method("__len", |_lua, this, ()| Ok(len(&this.0)));

    // TODO: replace shallow dump with new implementation used in `Table`?
    methods.add_meta_method("__pairs", |lua, this, ()| {
      let next: mlua::Value = lua.globals().raw_get("next")?;
      let table = this.0.shallow_dump(lua)?;
      Ok((next, table, mlua::Value::Nil))
    });
  }
}

impl Serialize for SharedTableScope {
  fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
    self.0.serialize(ser)
  }
}

#[derive(Default)]
struct SharedTableRepr {
  int: BTreeMap<i64, SharedTableValue>,
  hash: HashMap<SharedTableKey, SharedTableValue>,
  array: bool,
}

impl SharedTableRepr {
  fn get(&self, key: SharedTableKey) -> &SharedTableValue {
    const CONST_NIL: SharedTableValue = SharedTableValue::Nil;

    (key.to_i64())
      .map(|i| self.int.get(&i))
      .unwrap_or_else(|| self.hash.get(&key))
      .unwrap_or(&CONST_NIL)
  }

  fn set(&mut self, key: SharedTableKey, value: SharedTableValue) -> SharedTableValue {
    if let Some(i) = key.to_i64() {
      self.int.insert(i, value)
    } else {
      self.hash.insert(key, value)
    }
    .unwrap_or(SharedTableValue::Nil)
  }

  fn shallow_dump<'lua>(&self, lua: &'lua Lua) -> mlua::Result<Table<'lua>> {
    let int_iter = (self.int)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let hash_iter = (self.hash)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let t = lua.create_table_with_capacity(self.int.len() as _, self.hash.len() as _)?;
    for kv in int_iter.chain(hash_iter) {
      let (k, v) = kv?;
      t.raw_set(k, v)?;
    }
    Ok(t)
  }

  fn _deep_dump<'lua>(
    &self,
    lua: &'lua Lua,
    ptr: usize,
    tables: &mut HashMap<usize, Table<'lua>>,
  ) -> mlua::Result<Table<'lua>> {
    let int_iter = (self.int)
      .iter()
      .map(|(i, v)| Ok::<_, mlua::Error>((mlua::Value::Integer(*i), v)));
    let other_iter = (self.hash)
      .iter()
      .map(|(k, v)| Ok::<_, mlua::Error>((k.to_lua(lua)?, v)));
    let table = lua.create_table_with_capacity(self.int.len() as _, self.hash.len() as _)?;
    if self.array {
      table.set_metatable(Some(lua.array_metatable()));
    }
    tables.insert(ptr, table.clone());
    for kv in int_iter.chain(other_iter) {
      let (k, v) = kv?;
      if let SharedTableValue::Table(x @ SharedTable(_)) = v {
        let sub_table = SharedTable::_deep_dump(x, lua, tables)?;
        table.raw_set(k, sub_table)?;
      } else {
        table.raw_set(k, v)?;
      }
    }
    Ok(table)
  }
}

impl Serialize for SharedTableRepr {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    if self.int.contains_key(&1) || self.array {
      let mut seq = serializer.serialize_seq(None)?;
      let mut prev = 0;
      for (&i, v) in self.int.iter() {
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
      self.hash.serialize(serializer)
    }
  }
}

fn len(x: &SharedTableRepr) -> i64 {
  x.int.iter().last().map(|x| 0.max(*x.0)).unwrap_or(0)
}
