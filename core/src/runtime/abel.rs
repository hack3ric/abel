use crate::lua::error::{
  arg_error, check_integer, check_userdata_mut, check_value, rt_error, tag_error, tag_handler,
};
use crate::lua::LuaCacheExt;
use crate::task::{LocalTask, TaskContext};
use futures::future::BoxFuture;
use futures::{Future, FutureExt};
use mlua::Value::Nil;
use mlua::{Function, Lua, MultiValue, RegistryKey, Table, UserData};
use std::time::Duration;
use tokio::sync::oneshot::error::RecvError;

pub fn side_effect_abel(lua: &Lua, local_env: Table, internal: Table) -> mlua::Result<()> {
  use mlua::Value::Function as Func;
  let abel = lua.create_table_from([
    ("listen", Func(create_fn_listen(lua, internal)?)),
    ("spawn", Func(create_fn_spawn(lua)?)),
    ("await_all", Func(create_fn_await_all(lua)?)),
    ("sleep", Func(create_fn_sleep(lua)?)),
    ("current_worker", lua.pack(std::thread::current().name())?),
  ])?;
  local_env.raw_set("abel", abel.clone())?;
  Ok(())
}

fn create_fn_listen<'a>(lua: &'a Lua, internal: Table<'a>) -> mlua::Result<Function<'a>> {
  const SRC: &str = r#"
    local internal, path, handler = ...
    assert(
      not internal.sealed,
      "cannot call `listen` from places other than the top level of `main.lua`"
    )
    local type_handler = type(handler)
    if type_handler ~= "function" then
      if type_handler == "table" then
        local mt = getmetatable(handler)
        if type(mt) == "table" and type(mt.__call) == "function" then
          goto ok
        end
      end
      error "handler must either be a function or a callable table"
    end

    ::ok::
    table.insert(internal.paths, { path, handler })
  "#;
  let f = lua.create_cached_value("abel:abel.listen::meta", || {
    lua.load(SRC).set_name("@[abel.listen]")?.into_function()
  })?;
  f.bind(internal)
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

pub(crate) fn abel_spawn(
  lua: &Lua,
  f: Function,
) -> mlua::Result<impl Future<Output = Result<Box<mlua::Result<RegistryKey>>, RecvError>> + Send> {
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
  Ok(rx)
}

pub(crate) fn create_fn_spawn(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:abel.spawn", |lua, mut args: MultiValue| {
    let f: Function =
      check_value(lua, args.pop_front(), "function").map_err(tag_handler(lua, 1, 1))?;
    let f = if args.is_empty() { f } else { f.bind(args)? };
    let rx = abel_spawn(lua, f)?;
    Ok(LuaPromise { inner: rx.boxed() })
  })
}

fn create_fn_await_all(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_async_function("abel:abel.await_all", |lua, args: MultiValue| async move {
    let args = args
      .into_iter()
      .enumerate()
      .map(|(i, x)| match x {
        mlua::Value::UserData(u) => {
          if let Ok(p) = u.take::<LuaPromise>() {
            Ok(p.inner)
          } else {
            Err(tag_error(lua, i + 1, "Promise", "other userdata", 1))
          }
        }
        mlua::Value::Function(f) => abel_spawn(lua, f).map(FutureExt::boxed),
        #[rustfmt::skip]
        _ => Err(tag_error(lua, i + 1, "Promise or function", x.type_name(), 1)),
      })
      .collect::<mlua::Result<Vec<_>>>()?;
    let mut result = futures::future::join_all(args).await;
    let mut mv = result
      .pop()
      .map(|x| {
        lua
          .registry_value::<Table>(&(*x.map_err(rt_error)?)?)?
          .raw_sequence_values()
          .collect::<mlua::Result<MultiValue>>()
      })
      .unwrap_or(Ok(MultiValue::new()))?;
    for x in result.into_iter().rev() {
      let table = lua.registry_value::<Table>(&(*x.map_err(rt_error)?)?)?;
      let value = table.raw_get(1).unwrap_or(Nil);
      mv.push_front(value)
    }
    Ok(mv)
  })
}

fn create_fn_sleep(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_async_function("abel:abel.sleep", |lua, mut args: MultiValue| async move {
    let ms = check_integer(args.pop_front()).map_err(tag_handler(lua, 1, 1))?;
    let ms =
      u64::try_from(ms).map_err(|_| arg_error(lua, 1, "sleep time cannot be negative", 1))?;
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Ok(())
  })
}
