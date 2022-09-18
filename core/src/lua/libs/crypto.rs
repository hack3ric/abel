use crate::lua::error::{check_string, check_userdata_mut, rt_error, tag_handler};
use crate::lua::LuaCacheExt;
use data_encoding::HEXLOWER;
use digest::Digest;
use mlua::{Function, Lua, MultiValue, UserData};
use sha2::{Sha224, Sha256, Sha384, Sha512, Sha512_224, Sha512_256};

pub fn create_preload_crypto(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:preload_crypto", |lua, ()| {
    let crypto_table = lua.create_table()?;
    crypto_table.raw_set("Sha224", create_digest_interface::<Sha224>(lua)?)?;
    crypto_table.raw_set("Sha256", create_digest_interface::<Sha256>(lua)?)?;
    crypto_table.raw_set("Sha384", create_digest_interface::<Sha384>(lua)?)?;
    crypto_table.raw_set("Sha512", create_digest_interface::<Sha512>(lua)?)?;
    crypto_table.raw_set("Sha512_224", create_digest_interface::<Sha512_224>(lua)?)?;
    crypto_table.raw_set("Sha512_256", create_digest_interface::<Sha512_256>(lua)?)?;
    Ok(crypto_table)
  })
}

struct LuaHasher<H: Digest + 'static>(Option<H>);

impl<H: Digest + 'static> UserData for LuaHasher<H> {
  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_function("write", |lua, mut args: MultiValue| {
      let mut this =
        check_userdata_mut::<Self>(args.pop_front(), "hasher").map_err(tag_handler(lua, 1, 0))?;
      let bytes = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 2, 0))?;
      if let Some(inner) = &mut this.with_borrowed_mut(|x| &mut x.0) {
        inner.update(bytes.as_bytes());
        Ok(())
      } else {
        Err(rt_error("attempt to update a hasher after finalizing"))
      }
    });

    // TODO: output format
    methods.add_function("finalize", |lua, mut args: MultiValue| {
      let mut this =
        check_userdata_mut::<Self>(args.pop_front(), "hasher").map_err(tag_handler(lua, 1, 0))?;
      if let Some(inner) = this.with_borrowed_mut(|x| &mut x.0).take() {
        let out = inner.finalize();
        lua.create_string(&HEXLOWER.encode(&out))
      } else {
        Err(rt_error("attempt to finalize a hasher after finalizing"))
      }
    });
  }
}

fn create_digest_interface<H: Digest + 'static>(lua: &Lua) -> mlua::Result<Function> {
  lua.create_function(|lua, mut args: MultiValue| {
    if args.is_empty() {
      lua.pack(LuaHasher(Some(H::new())))
    } else {
      let data = check_string(lua, args.pop_front()).map_err(tag_handler(lua, 1, 0))?;
      let out = H::digest(data);
      let out = lua.create_string(&HEXLOWER.encode(&out))?;
      Ok(mlua::Value::String(out))
    }
  })
}
