use super::header_name;
use crate::lua::error::{arg_error, check_arg, check_userdata, rt_error_fmt};
use hyper::header::HeaderValue;
use hyper::HeaderMap;
use mlua::{AnyUserData, MultiValue, UserData, UserDataMethods, Variadic};
use ouroboros::self_referencing;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

pub struct LuaHeaderMap(pub(crate) Rc<RefCell<HeaderMap>>);

impl UserData for LuaHeaderMap {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_function("get", |lua, args: MultiValue| {
      let this = check_userdata::<Self>(lua, &args, 1, "header map", 0)?;
      let name: mlua::String = check_arg(lua, &args, 2, "string", 0)?;
      let name = header_name(name).map_err(|_| arg_error(lua, 2, "invalid header name", 0))?;
      let header_map = this.0.borrow();
      header_map
        .get_all(name)
        .into_iter()
        .map(|x| lua.create_string(x.as_bytes()))
        .collect::<mlua::Result<Variadic<_>>>()
    });

    methods.add_meta_method("__index", |lua, this, name: mlua::Value| {
      let type_name = name.type_name();
      let name: mlua::String = lua
        .unpack(name)
        .map_err(|_| rt_error_fmt!("cannot index header map with {type_name}"))?;
      (this.0)
        .borrow()
        .get(header_name(name)?)
        .map(|x| lua.create_string(x.as_bytes()))
        .transpose()
    });

    methods.add_meta_method("__pairs", |lua, this, ()| {
      let iter = LuaHeaderMapIterBuilder {
        inner: this.0.clone(),
        borrow_builder: |x| x.borrow_mut(),
        iter_builder: |x| x.iter(),
      }
      .build();

      let iter_fn = lua.create_function(|lua, iter: AnyUserData| {
        let mut iter = iter.borrow_mut::<LuaHeaderMapIter>()?;
        let result = iter
          .with_iter_mut(|x| x.next())
          .map(|(k, v)| {
            mlua::Result::Ok(Variadic::from_iter([
              lua.create_string(k.as_str())?,
              lua.create_string(v.as_bytes())?,
            ]))
          })
          .transpose()?
          .unwrap_or_else(Variadic::new);
        Ok(result)
      })?;

      iter_fn.bind(iter)
    });
  }
}

#[self_referencing]
struct LuaHeaderMapIter {
  inner: Rc<RefCell<HeaderMap>>,

  #[borrows(inner)]
  #[not_covariant]
  borrow: RefMut<'this, HeaderMap>,

  #[borrows(borrow)]
  #[covariant]
  iter: hyper::header::Iter<'this, HeaderValue>,
}

impl UserData for LuaHeaderMapIter {}
