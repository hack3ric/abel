use dashmap::DashMap;
use mlua::{ExternalResult, LuaSerdeExt, String as LuaString, UserData, Lua, Function};
use once_cell::sync::Lazy;
use std::sync::{Arc, Weak};

type Key = Box<str>;
type Value = serde_json::Value;
type Context = DashMap<Key, Value>;
type ContextStore = Arc<DashMap<ContextStoreKey, Arc<Context>>>;

#[derive(Debug, Hash, PartialEq, Eq)]
struct ContextStoreKey {
  pub service_name: Box<str>,
  pub name: Box<str>,
}

static CONTEXT_STORE: Lazy<ContextStore> = Lazy::new(|| Arc::new(DashMap::new()));

struct ContextRef {
  inner: Weak<Context>,
}

impl ContextRef {
  fn get(&self) -> mlua::Result<Arc<Context>> {
    self
      .inner
      .upgrade()
      .ok_or_else(|| "context is already dropped")
      .to_lua_err()
  }
}

impl UserData for ContextRef {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_method("get", |lua, this, key: LuaString| {
      this
        .get()?
        .get(key.to_str()?)
        .map(|x| lua.to_value(x.value()))
        .unwrap_or(Ok(mlua::Value::Nil))
    });

    methods.add_method("set", |lua, this, (key, value): (LuaString, mlua::Value)| {
      this
        .get()?
        .insert(key.to_str()?.into(), lua.from_value(value)?);
      Ok(())
    });
  }
}

pub fn create_fn_context<'a>(lua: &'a Lua, service_name: Box<str>) -> mlua::Result<Function<'a>> {
  lua.create_function(move |_lua, name: String| {
    let store_key = ContextStoreKey { service_name: service_name.clone(), name: name.into_boxed_str() };
    let x = CONTEXT_STORE.entry(store_key).or_insert_with(|| Arc::new(DashMap::new()));
    Ok(ContextRef { inner: Arc::downgrade(x.value()) })
  })
}

pub fn remove_service_contexts(service_name: &str) {
  CONTEXT_STORE.retain(|k, _| &*k.service_name != service_name);
}
