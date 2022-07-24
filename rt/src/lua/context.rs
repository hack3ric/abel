// TODO: Move context into Sandbox?

use super::LuaCacheExt;
use mlua::{Lua, RegistryKey, Table, ToLua};
use parking_lot::Mutex;
use std::cell::Ref;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct TaskContext {
  pub close_table: Option<Rc<RegistryKey>>,
  pub cpu_time: Arc<Mutex<Duration>>,
}

impl TaskContext {
  pub fn new_with_close_table(lua: &Lua) -> mlua::Result<Self> {
    let close_table = lua.create_registry_value(lua.create_table()?)?;
    Ok(Self {
      close_table: Some(Rc::new(close_table)),
      ..Default::default()
    })
  }

  pub fn set_current(&self, lua: &Lua) {
    lua.set_app_data(self.clone());
  }

  pub fn get_current(lua: &Lua) -> Option<Ref<Self>> {
    lua.app_data_ref::<Self>()
  }

  pub fn remove_current(lua: &Lua) -> Option<Self> {
    lua.remove_app_data::<Self>()
  }

  pub fn try_close(&mut self, lua: &Lua) -> mlua::Result<()> {
    if let Some(context) = self.close_table.take().and_then(|x| Rc::try_unwrap(x).ok()) {
      let context_table: Table = lua.registry_value(&context)?;
      lua.remove_registry_value(context)?;
      lua
        .create_cached_value("abel:context_try_close", |lua| {
          let code = r#"
          local context_table = ...
          for _, v in ipairs(context_table) do
            pcall(function()
              local _ <close> = v
            end)
          end
          "#;
          lua
            .load(code)
            .set_name("abel_destroy_context")?
            .into_function()
        })?
        .call(context_table)?;
    }
    Ok(())
  }

  pub fn register<'lua, T: ToLua<'lua>>(lua: &'lua Lua, value: T) -> mlua::Result<()> {
    if let Some(ctx) = Self::get_current(lua) {
      if let Some(close_table) = &ctx.close_table {
        let context: Table = lua.registry_value(close_table)?;
        context.raw_insert(context.raw_len() + 1, value)?;
      }
    }
    Ok(())
  }
}

impl PartialEq for TaskContext {
  fn eq(&self, other: &Self) -> bool {
    self.close_table == other.close_table && Arc::ptr_eq(&self.cpu_time, &other.cpu_time)
  }
}

// pub fn create(lua: &Lua) -> mlua::Result<RegistryKey> {
//   lua.create_registry_value(lua.create_table()?)
// }

// pub fn set_current(lua: &Lua, context: Option<&RegistryKey>) ->
// mlua::Result<()> {   let context = context
//     .map(|x| lua.registry_value::<Table>(x))
//     .transpose()?;
//   lua.set_named_registry_value("abel_current_context", context)
// }

// pub fn destroy(lua: &Lua, context: RegistryKey) -> mlua::Result<()> {
//   let context_table: Table = lua.registry_value(&context)?;
//   lua.remove_registry_value(context)?;
//   let code = mlua::chunk! {
//     for _, v in ipairs($context_table) do
//       pcall(function()
//         local _ <close> = v
//       end)
//     end
//   };
//   lua.load(code).set_name("abel_destroy_context")?.call(())
// }

// pub fn register<'lua>(lua: &'lua Lua, value: impl ToLua<'lua>) ->
// mlua::Result<()> {   let context: Table =
// lua.named_registry_value("abel_current_context")?;
//   context.raw_insert(context.raw_len() + 1, value)
// }
