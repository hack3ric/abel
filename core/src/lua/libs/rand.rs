use crate::lua::error::{arg_error, check_integer, check_userdata_mut, tag_handler};
use crate::lua::LuaCacheExt;
use mlua::{Function, Lua, MultiValue, UserData};
use rand::{thread_rng, Rng, RngCore};

struct LuaRng(Box<dyn RngCore>);

impl UserData for LuaRng {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_function("random", |lua, mut args: MultiValue| {
      let mut this =
        check_userdata_mut::<Self>(args.pop_front(), "RNG").map_err(tag_handler(lua, 1, 0))?;
      this.with_borrowed_mut(|r| Ok(r.0.gen::<f64>()))
    });

    methods.add_function("gen_range", |lua, mut args: MultiValue| {
      let mut this =
        check_userdata_mut::<Self>(args.pop_front(), "RNG").map_err(tag_handler(lua, 1, 0))?;
      let low = check_integer(args.pop_front()).map_err(tag_handler(lua, 2, 0))?;
      let high = check_integer(args.pop_front()).map_err(tag_handler(lua, 3, 0))?;

      if low >= high {
        Err(arg_error(lua, 3, "range is empty", 0))
      } else {
        this.with_borrowed_mut(|r| Ok(r.0.gen_range(low..=high)))
      }
    });
  }
}

pub fn create_preload_rand(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:preload_rand", |lua, ()| {
    let rand_table = lua.create_table()?;
    rand_table.raw_set(
      "ThreadRng",
      lua.create_cached_value("abel:rand.ThreadRng", || {
        lua.create_userdata(LuaRng(Box::new(thread_rng())))
      })?,
    )?;
    Ok(rand_table)
  })
}
