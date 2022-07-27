use super::error::{check_userdata_mut, check_value, rt_error, tag_handler};
use super::LuaCacheExt;
use crate::task::{LocalTask, TaskContext};
use futures::future::BoxFuture;
use futures::FutureExt;
use mlua::{Function, Lua, MultiValue, RegistryKey, Table, UserData};
use tokio::sync::oneshot::error::RecvError;

pub fn create_fn_spawn(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:spawn", |lua, mut args: MultiValue| {
    let f: Function =
      check_value(lua, args.pop_front(), "function").map_err(tag_handler(lua, 1, 1))?;
    let f = if args.is_empty() { f } else { f.bind(args)? };
    let key = lua.create_registry_value(f)?;
    let ctx = TaskContext::get_current(lua)
      .map(|x| x.clone())
      .unwrap_or_default();
    let (task, rx) = LocalTask::new(ctx, |rt| async move {
      let lua = rt.lua();
      let f: Function = lua.registry_value(&key)?;
      let result: MultiValue = f.call_async(()).await?;
      let table = lua.create_sequence_from(result)?;
      lua.create_registry_value(table)
    });
    {
      let mut x = lua.app_data_mut::<Vec<LocalTask>>().unwrap();
      x.push(task);
    }
    Ok(LuaPromise { inner: rx.boxed() })
  })
}

pub struct LuaPromise {
  inner: BoxFuture<'static, Result<Box<mlua::Result<RegistryKey>>, RecvError>>,
}

impl UserData for LuaPromise {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_function("await", |lua, mut args: MultiValue| async move {
      let mut this =
        check_userdata_mut::<Self>(args.pop_front(), "Promise").map_err(tag_handler(lua, 1, 1))?;
      let result = this
        .with_borrowed_mut(|x| &mut x.inner)
        .await
        .map_err(rt_error)?;
      lua
        .registry_value::<Table>(&(*result)?)?
        .raw_sequence_values()
        .collect::<mlua::Result<MultiValue>>()
    })
  }
}
