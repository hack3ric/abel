use super::{len, SharedTable, SharedTableScope};
use crate::lua::BadArgument;
use mlua::{Function, Lua, Table};

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
        Err(userdata_not_shared_table("dump", 1))
      }
    }
    _ => Err(expected_table("dump", 1, table.type_name())),
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
          Err(userdata_not_shared_table("scope", 1))
        }
      }
      _ => Err(expected_table("scope", 1, table.type_name())),
    }
  })
}

fn create_fn_table_insert_shared_2(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, (table, value): (mlua::AnyUserData, mlua::Value)| {
    if let Ok(table) = table.borrow::<SharedTable>() {
      table.push(lua.unpack(value)?);
      Ok(())
    } else {
      Err(userdata_not_shared_table("insert", 1))
    }
  })
}

fn create_fn_table_insert_shared_3(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(
    |lua, (table, pos, value): (mlua::AnyUserData, i64, mlua::Value)| {
      if pos < 1 {
        return Err(out_of_bounds("insert", 2));
      }
      if let Ok(table) = table.borrow::<SharedTable>() {
        let mut lock = table.0.write();
        if pos > len(&lock) + 1 {
          return Err(out_of_bounds("insert", 2));
        }
        let right = lock.int.split_off(&pos);
        let iter = right.into_iter().map(|(i, v)| (i + 1, v));
        lock.int.insert(pos, lua.unpack(value)?);
        lock.int.extend(iter);
      } else if let Ok(mut table) = table.borrow_mut::<SharedTableScope>() {
        if pos > len(&table.0) + 1 {
          return Err(out_of_bounds("insert", 2));
        }
        let right = table.0.int.split_off(&pos);
        let iter = right.into_iter().map(|(i, v)| (i + 1, v));
        (table.0.int).insert(pos, lua.unpack(value)?);
        table.0.int.extend(iter);
      } else {
        return Err(userdata_not_shared_table("insert", 1));
      }
      Ok(())
    },
  )
}

// TODO: replace it with Rust implementation
fn create_fn_table_insert(lua: &Lua) -> mlua::Result<Function> {
  let table_insert_shared_2 = create_fn_table_insert_shared_2(lua)?;
  let table_insert_shared_3 = create_fn_table_insert_shared_3(lua)?;

  let table_insert = mlua::chunk! {
    local old_table_insert = table.insert
    local function insert(t, ...)
      if type(t) == "table" then
        return old_table_insert(t, ...)
      elseif type(t) == "userdata" then
        local len = select("#", ...)
        if len == 1 then
          // t[#t + 1] = ... // this caused data racing
          $table_insert_shared_2(t, ...)
        elseif len == 2 then
          $table_insert_shared_3(t, ...)
        else
          error "wrong number of arguments"
        end
      else
        error("expected table or shared table, got " .. type(t))
      end
    end
    return insert
  };
  lua.load(table_insert).call(())
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

fn out_of_bounds(fn_name: &'static str, pos: u8) -> mlua::Error {
  BadArgument::new(fn_name, pos, "out of bounds").into()
}
