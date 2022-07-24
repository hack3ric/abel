use super::error::{check_userdata_mut, check_value, rt_error, tag_handler};
use super::LuaCacheExt;
use crate::{OwnedTask, Sandbox, Task};
use futures::future::BoxFuture;
use mlua::{Function, Lua, MultiValue, RegistryKey, Table, UserData};
use std::ops::Deref;
use tokio::sync::mpsc;
use tokio::sync::oneshot::error::RecvError;

pub fn create_fn_spawn<R: Deref<Target = Sandbox<E>>, E>(
  lua: &Lua,
  task_tx: mpsc::Sender<Task<R>>,
) -> mlua::Result<Function> {
  lua.create_cached_async_function("abel:spawn", move |lua, mut args: MultiValue| {
    let task_tx = task_tx.clone();
    async move {
      let f: Function =
        check_value(lua, args.pop_front(), "function").map_err(tag_handler(lua, 1, 1))?;
      // HACK: `Function::bind` is still somehow buggy. Fixed in the next version.
      // let f = f.bind(args)?;
      let f = if args.is_empty() { f } else { f.bind(args)? };
      let key = lua.create_registry_value(f)?;
      let (task, rx) = OwnedTask::<R>::new(move |rt| async move {
        let lua = rt.lua();
        let f: Function = lua.registry_value(&key)?;
        let result: MultiValue = f.call_async(()).await?;
        lua.create_registry_value(lua.create_sequence_from(result)?)
      });
      task_tx
        .send(task.into())
        .await
        .map_err(|_| rt_error("failed to spawn"))?;
      Ok(Promise {
        inner: Box::pin(rx),
      })
    }
  })
}

struct Promise {
  inner: BoxFuture<'static, Result<Box<mlua::Result<RegistryKey>>, RecvError>>,
}

impl UserData for Promise {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_async_function("await", |lua, mut args: MultiValue| async move {
      let mut this =
        check_userdata_mut::<Self>(args.pop_front(), "promise").map_err(tag_handler(lua, 1, 1))?;
      let result = this
        .with_borrowed_mut(|x| &mut x.inner)
        .await
        .map_err(rt_error)?;
      let result: Table = lua.registry_value(&(*result)?)?;
      result
        .raw_sequence_values()
        .collect::<mlua::Result<MultiValue>>()
    })
  }
}
