use error_chain::error_chain;

error_chain! {
  types {
    Error, ErrorKind, ResultExt, HiveResult;
  }

  foreign_links {
    Lua(mlua::Error);
  }
}
