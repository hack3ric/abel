use mlua::{ExternalError, Function, Lua, UserData};
use rand::{thread_rng, Rng, RngCore};

struct LuaRng(Box<dyn RngCore>);

impl UserData for LuaRng {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_method_mut("random", |_lua, this, ()| Ok(this.0.gen::<f64>()));

    methods.add_method_mut("gen_range", |_lua, this, (low, high): (i64, i64)| {
      if low >= high {
        Err("range is empty".to_lua_err())
      } else {
        Ok(this.0.gen_range(low..=high))
      }
    })
  }
}

pub fn create_preload_crypto(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, ()| {
    let crypto_table = lua.create_table()?;
    crypto_table.raw_set("thread_rng", LuaRng(Box::new(thread_rng())))?;
    Ok(crypto_table)
  })
}
