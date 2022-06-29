use super::error::{arg_error, check_arg, check_userdata_mut};
use mlua::{Function, Lua, MultiValue, UserData};
use rand::{thread_rng, Rng, RngCore};

struct LuaRng(Box<dyn RngCore>);

impl UserData for LuaRng {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_function("random", |lua, args: MultiValue| {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "LuaRng", 0)?;
      Ok(this.0.gen::<f64>())
    });

    methods.add_function("gen_range", |lua, args: MultiValue| {
      let mut this = check_userdata_mut::<Self>(lua, &args, 1, "LuaRng", 0)?;
      let low: i64 = check_arg(lua, &args, 2, "integer", 0)?;
      let high: i64 = check_arg(lua, &args, 3, "integer", 0)?;

      if low >= high {
        Err(arg_error(lua, 3, "range is empty", 0))
      } else {
        Ok(this.0.gen_range(low..=high))
      }
    });
  }
}

pub fn create_preload_crypto(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| {
    let crypto_table = lua.create_table()?;
    crypto_table.raw_set("thread_rng", LuaRng(Box::new(thread_rng())))?;
    Ok(crypto_table)
  })
}
