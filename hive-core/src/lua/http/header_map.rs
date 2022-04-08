use hyper::header::{HeaderName, HeaderValue};
use hyper::HeaderMap;
use mlua::{AnyUserData, ExternalResult, UserData, UserDataMethods, Variadic};
use ouroboros::self_referencing;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

pub struct LuaHeaderMap(pub(crate) Rc<RefCell<HeaderMap>>);

impl UserData for LuaHeaderMap {
  fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
    fn header_name(name: mlua::String) -> mlua::Result<HeaderName> {
      HeaderName::from_bytes(name.as_bytes()).to_lua_err()
    }

    methods.add_method("get", |lua, this, name: mlua::String| {
      (this.0)
        .borrow()
        .get_all(header_name(name)?)
        .into_iter()
        .map(|x| lua.create_string(x.as_bytes()))
        .collect::<mlua::Result<Variadic<_>>>()
    });

    methods.add_meta_method("__index", |lua, this, name: mlua::String| {
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
