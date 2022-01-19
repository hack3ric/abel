use dashmap::DashMap;
use mlua::{ExternalResult, Function, Lua, LuaSerdeExt, String as LuaString, UserData};
use once_cell::sync::Lazy;
use std::sync::{Arc, Weak};
use mlua::Value::Nil;

type Key = Box<str>;
type Context = DashMap<Key, serde_json::Value>;
type ContextStore = Arc<DashMap<ContextStoreKey, Arc<Context>>>;

#[derive(Debug, Hash, PartialEq, Eq)]
struct ContextStoreKey {
  pub service_name: Box<str>,
  pub name: Box<str>,
}

static CONTEXT_STORE: Lazy<ContextStore> = Lazy::new(|| Arc::new(DashMap::new()));

#[derive(Debug, Clone)]
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
        .unwrap_or(Ok(Nil))
    });

    methods.add_async_method(
      "set",
      |lua, this, (key, value): (LuaString, mlua::Value)| async move {
        let this = this.get()?;
        let key = key.to_str()?;
        if let mlua::Value::Function(f) = value {
          if let Some(mut r) = this.get_mut(key) {
            let old_value = lua.to_value(r.value())?;
            let new_value: mlua::Value = f.call_async(old_value.clone()).await?;
            *r.value_mut() = lua.from_value(new_value.clone())?;
            Ok((old_value, new_value))
          } else {
            let result: mlua::Value = f.call_async(Nil).await?;
            this.insert(key.into(), lua.from_value(result.clone())?);
            Ok((Nil, result))
          }
        } else {
          let result = this.insert(key.into(), lua.from_value(value.clone())?);
          Ok((result.map(|old_value| lua.to_value(&old_value)).unwrap_or(Ok(Nil))?, value))
        }
      },
    );
  }
}

pub fn create_fn_context<'a>(lua: &'a Lua, service_name: Box<str>) -> mlua::Result<Function<'a>> {
  lua.create_function(move |_lua, name: String| {
    let store_key = ContextStoreKey {
      service_name: service_name.clone(),
      name: name.into_boxed_str(),
    };
    let x = CONTEXT_STORE
      .entry(store_key)
      .or_insert_with(|| Arc::new(DashMap::new()));
    Ok(ContextRef {
      inner: Arc::downgrade(x.value()),
    })
  })
}

pub fn remove_service_contexts(service_name: &str) {
  CONTEXT_STORE.retain(|k, _| &*k.service_name != service_name);
}
