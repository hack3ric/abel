use super::{len, SharedTable, SharedTableScope};
use crate::lua::error::{arg_error, lua_error, tag_error};
use crate::lua::LuaTableExt;
use mlua::{AnyUserData, Function, Lua, MultiValue, Table};

pub fn apply_table_module_patch(lua: &Lua, table_module: Table) -> mlua::Result<()> {
  table_module.raw_set("dump", create_fn_table_dump(lua)?)?;
  table_module.raw_set("insert", create_fn_table_insert(lua)?)?;
  table_module.raw_set("scope", create_fn_table_scope(lua)?)?;
  Ok(())
}

fn create_fn_table_dump(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, table: mlua::Value| match table {
    mlua::Value::Table(table) => Ok(table),
    mlua::Value::UserData(x) => {
      if let Ok(x) = x.borrow::<SharedTable>() {
        x.deep_dump(lua)
      } else if let Ok(x) = x.borrow::<SharedTableScope>() {
        x.deep_dump(lua)
      } else {
        Err(userdata_not_shared_table(lua, 1))
      }
    }
    _ => Err(expected_table(lua, 1, table.type_name())),
  })
}

fn create_fn_table_scope(lua: &Lua) -> mlua::Result<Function> {
  lua.create_async_function(|lua, (table, f): (mlua::Value, Function)| async move {
    match table {
      mlua::Value::Table(table) => f.call_async(table).await,
      mlua::Value::UserData(x) => {
        if let Ok(x) = x.borrow::<SharedTable>() {
          let x = lua.create_ser_userdata(SharedTableScope::new(x.0.clone()))?;
          let result = f.call_async::<_, mlua::Value>(x.clone()).await;
          x.take::<SharedTableScope>()?;
          return result;
        }
        if x.borrow::<SharedTableScope>().is_ok() {
          f.call_async::<_, mlua::Value>(x).await
        } else {
          Err(userdata_not_shared_table(lua, 1))
        }
      }
      _ => Err(expected_table(lua, 1, table.type_name())),
    }
  })
}

fn table_insert_shared_2(lua: &Lua, table: AnyUserData, value: mlua::Value) -> mlua::Result<()> {
  let (borrowed, owned);
  let table = if let Ok(table) = table.borrow::<SharedTable>() {
    owned = SharedTableScope::new(table.0.clone());
    &owned
  } else if let Ok(table) = table.borrow::<SharedTableScope>() {
    borrowed = table;
    &borrowed
  } else {
    return Err(userdata_not_shared_table(lua, 1));
  };

  table.push(lua.unpack(value)?);
  Ok(())
}

fn table_insert_shared_3(
  lua: &Lua,
  table: AnyUserData,
  pos: i64,
  value: mlua::Value,
) -> mlua::Result<()> {
  if pos < 1 {
    return Err(out_of_bounds(lua, 2));
  }
  let (borrowed, owned);
  let table = if let Ok(table) = table.borrow::<SharedTable>() {
    owned = SharedTableScope::new(table.0.clone());
    &owned
  } else if let Ok(table) = table.borrow::<SharedTableScope>() {
    borrowed = table;
    &borrowed
  } else {
    return Err(userdata_not_shared_table(lua, 1));
  };

  let mut guard = table.0.borrow_mut();
  if pos > len(&guard) + 1 {
    return Err(out_of_bounds(lua, 2));
  }
  let right = guard.int.split_off(&pos);
  let iter = right.into_iter().map(|(i, v)| (i + 1, v));
  (guard.int).insert(pos, lua.unpack(value)?);
  guard.int.extend(iter);

  Ok(())
}

fn create_fn_table_insert(lua: &Lua) -> mlua::Result<Function> {
  let old: Function = lua
    .globals()
    .raw_get_path("<global>", &["table", "insert"])?;
  let f = lua.create_function(
    |lua, (old, table, args): (Function, mlua::Value, MultiValue)| match table {
      mlua::Value::Table(table) => old.call::<_, ()>((table, args)),
      mlua::Value::UserData(table) => {
        let mut args = args.into_iter();
        match args.len() {
          1 => table_insert_shared_2(lua, table, args.next().unwrap()),
          2 => table_insert_shared_3(
            lua,
            table,
            lua.unpack(args.next().unwrap())?,
            args.next().unwrap(),
          ),
          _ => Err(lua_error("wrong number of arguments")),
        }
      }
      _ => Err(expected_table(lua, 1, table.type_name())),
    },
  )?;
  f.bind(old)
}

// Error utilities

fn userdata_not_shared_table(lua: &Lua, pos: usize) -> mlua::Error {
  arg_error(lua, pos, "failed to borrow userdata as shared table", 0)
}

fn expected_table(lua: &Lua, pos: usize, found: &str) -> mlua::Error {
  tag_error(lua, pos, "table or shared table", found, 0)
}

fn out_of_bounds(lua: &Lua, pos: usize) -> mlua::Error {
  arg_error(lua, pos, "out of bounds", 0)
}
