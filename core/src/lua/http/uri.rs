use crate::lua::error::{
  check_userdata, check_value, rt_error, rt_error_fmt, tag_handler, TableCheckExt,
};
use crate::lua::{LuaCacheExt, LuaEither};
use bstr::ByteSlice;
use hyper::http::uri::{Authority, Parts, PathAndQuery, Scheme};
use hyper::Uri;
use mlua::{FromLua, Function, Lua, MultiValue, Table, ToLua, UserData};
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;

#[derive(Debug)]
pub struct LuaUri(pub(crate) Uri);

impl LuaUri {
  fn from_lua_parts(lua: &Lua, parts: Table) -> mlua::Result<Self> {
    let mut p = Parts::default();

    p.scheme = parts
      .check_raw_get::<Option<mlua::String>>(lua, "scheme", "string")?
      .map(|x| {
        Scheme::try_from(x.as_bytes())
          .map_err(|_| rt_error_fmt!("invalid scheme: '{}'", x.to_string_lossy()))
      })
      .transpose()?;

    p.authority = parts
      .check_raw_get::<Option<mlua::String>>(lua, "authority", "string")?
      .map(|x| {
        Authority::try_from(x.as_bytes())
          .map_err(|error| rt_error_fmt!("invalid authority: '{}' ({error})", x.to_string_lossy()))
      })
      .transpose()?;

    let path_and_query: Option<mlua::String> =
      parts.check_raw_get(lua, "path_and_query", "string")?;
    p.path_and_query = if let Some(x) = path_and_query.as_ref() {
      Some(PathAndQuery::try_from(x.as_bytes()).map_err(|error| {
        rt_error_fmt!("invalid path and query '{}' ({error})", x.to_string_lossy())
      })?)
    } else {
      let path = parts.check_raw_get::<Option<mlua::String>>(lua, "path", "string")?;
      let query: Option<LuaEither<mlua::String, Table>> =
        parts.check_raw_get(lua, "query", "string or table")?;
      let query: Option<Cow<[u8]>> = match query.as_ref() {
        Some(LuaEither::Left(s)) => Some(s.as_bytes().into()),
        Some(LuaEither::Right(t)) => serde_qs::to_string(t)
          .map(|x| Some(x.into_bytes().into()))
          .map_err(|error| rt_error_fmt!("failed to serialize query ({error})"))?,
        None => None,
      };
      let paq: Option<Cow<[u8]>> = match (path.as_ref(), query) {
        (Some(p), Some(q)) => {
          let mut result = p.as_bytes().to_vec();
          result.push(b'?');
          result.extend(&*q);
          Some(result.into())
        }
        (Some(p), None) => Some(p.as_bytes().into()),
        (None, Some(q)) => {
          let result = std::iter::once(b'?')
            .chain(q.iter().copied())
            .collect::<Vec<u8>>();
          Some(result.into())
        }
        (None, None) => None,
      };
      paq
        .map(|x| {
          PathAndQuery::try_from(&*x)
            .map_err(|error| rt_error_fmt!("invalid path and query '{}' ({error})", x.as_bstr()))
        })
        .transpose()?
    };

    let uri = Uri::from_parts(p).map_err(rt_error)?;
    Ok(Self(uri))
  }
}

impl UserData for LuaUri {
  fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
    fields.add_field_method_get("scheme", |lua, this| lua.pack(this.0.scheme_str()));
    fields.add_field_method_get("host", |lua, this| lua.pack(this.0.host()));
    fields.add_field_method_get("port", |_lua, this| Ok(this.0.port_u16()));
    fields.add_field_method_get("authority", |lua, this| {
      lua.pack(this.0.authority().map(Authority::as_str))
    });
    fields.add_field_method_get("path", |lua, this| lua.pack(this.0.path()));
    fields.add_field_method_get("query_string", |lua, this| lua.pack(this.0.query()));
  }

  fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
    methods.add_meta_method("__tostring", |_lua, this, ()| Ok(this.0.to_string()));

    methods.add_function("query", |lua, mut args: MultiValue| {
      type QueryMap<'a> = HashMap<Cow<'a, str>, QueryField<'a>>;

      #[derive(Deserialize)]
      #[serde(untagged)]
      enum QueryField<'a> {
        Single(Cow<'a, str>),
        Map(QueryMap<'a>),
        Sequence(Vec<QueryField<'a>>),
      }

      impl<'a, 'lua> ToLua<'lua> for QueryField<'a> {
        fn to_lua(self, lua: &'lua Lua) -> mlua::Result<mlua::Value<'lua>> {
          match self {
            Self::Single(s) => lua.pack(s),
            Self::Map(x) => lua.pack(x),
            Self::Sequence(x) => lua.pack(x),
          }
        }
      }

      let this = check_userdata::<Self>(args.pop_front(), "URI").map_err(tag_handler(lua, 1, 0))?;
      (this.borrow_borrowed().0.query())
        .map(serde_qs::from_str::<QueryMap>)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(rt_error)
    });
  }
}

impl<'lua> FromLua<'lua> for LuaUri {
  fn from_lua(value: mlua::Value<'lua>, _lua: &'lua Lua) -> mlua::Result<Self> {
    match value {
      mlua::Value::String(s) => {
        Ok(Self(Uri::try_from(s.as_bytes()).map_err(|error| {
          rt_error_fmt!("failed to parse URI ({error})")
        })?))
      }
      mlua::Value::UserData(x) => {
        let x = x.borrow::<Self>()?;
        Ok(Self(x.0.clone()))
      }
      _ => Err(rt_error_fmt!(
        "failed to parse URI (string expected, got {})",
        value.type_name(),
      )),
    }
  }
}

pub fn create_fn_http_create_uri(lua: &Lua) -> mlua::Result<Function> {
  lua.create_cached_function("abel:http.Uri", |lua, mut args: MultiValue| {
    let s = check_value::<LuaEither<mlua::String, Table>>(lua, args.pop_front(), "string or table")
      .map_err(tag_handler(lua, 1, 0))?;
    match s {
      LuaEither::Left(s) => Ok(LuaUri(Uri::try_from(s.as_bytes()).map_err(rt_error)?)),
      LuaEither::Right(t) => LuaUri::from_lua_parts(lua, t),
    }
  })
}
