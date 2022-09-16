use super::error::resolve_callback_error;
use super::require::RemoteInterface;
use super::sandbox::Sandbox;
use crate::source::{Metadata, Source, SourceVfs};
use async_trait::async_trait;
use std::io::Cursor;
use tempfile::TempDir;
use tokio::io;

pub struct EmptySource;

#[async_trait]
impl SourceVfs for EmptySource {
  type File = Cursor<Vec<u8>>;

  async fn get(&self, _path: &str) -> io::Result<Self::File> {
    Err(io::Error::new(
      io::ErrorKind::NotFound,
      "No such file or directory",
    ))
  }

  async fn exists(&self, _path: &str) -> io::Result<bool> {
    Ok(false)
  }

  async fn metadata(&self, _path: &str) -> io::Result<Metadata> {
    Err(io::Error::new(
      io::ErrorKind::NotFound,
      "No such file or directory",
    ))
  }
}

macro_rules! run_lua_test {
  ($test_name:expr, $code:literal) => {
    async {
      if option_env!("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "INFO");
      }
      let _ = pretty_env_logger::try_init();
      let sandbox = Sandbox::new(RemoteInterface::new(None))?;
      let local_storage = TempDir::new()?;
      let isolate = sandbox
        .isolate_builder_with_stdlib(Source::new(EmptySource), local_storage.path())?
        .build()?;
      sandbox
        .run_isolate_ext::<_, _, ()>(&isolate, $code, $test_name, ())
        .await
    }
    .await
  };
}

fn error_to_string(error: &mlua::Error) -> String {
  match error {
    mlua::Error::CallbackError { traceback, cause } => {
      format!("{}\n{traceback}", resolve_callback_error(cause))
    }
    _ => error.to_string(),
  }
}

macro_rules! lua_tests {
  ($(
    $(#[$($attr:tt)*])*
    $test_name:ident $code:literal
  )*) => {
    $(
      $(#[$($attr)*])*
      #[tokio::test]
      async fn $test_name() {
        let result = run_lua_test! { std::stringify!($test_name), $code };
        if let Err(error) = result {
          panic!("{}", error_to_string(&error))
        }
      }
    )*
  };
}

lua_tests! {
  test_json r#"
    local json = require "json"
    local t = require "testing"

    t.assert_eq(assert(json.stringify {}), '{}')
    t.assert_eq(assert(json.stringify { "foo", "bar" }), '["foo","bar"]')
    t.assert_eq(assert(json.stringify(nil)), 'null')

    local table = {}
    t.assert_eq(assert(json.stringify(table)), '{}')
    t.assert_eq(assert(json.stringify(json.array(table))), '[]')
    t.assert_eq(assert(json.stringify(json.undo_array(table))), '{}')
  "#

  test_http_uri r#"
    local http = require "http"
    local t = require "testing"

    local uri = http.Uri "https://test.example.com:8080/path?foo=bar&baz=%20#fragment"

    t.assert_eq(uri.scheme, "https")
    t.assert_eq(uri.host, "test.example.com")
    t.assert_eq(uri.port, 8080)
    t.assert_eq(uri.path, "/path")
    t.assert_eq(uri.query_string, "foo=bar&baz=%20")

    -- Ignores fragment intentionally (see https://github.com/hyperium/hyper/issues/1345)
    t.assert_eq(
      tostring(uri),
      "https://test.example.com:8080/path?foo=bar&baz=%20"
    )

    local query = assert(uri:query())
    t.assert_eq(type(query), "table")
    t.assert_eq(query.foo, "bar")
    t.assert_eq(query.baz, " ")
  "#

  test_crypto_random r#"
    local crypto = require "crypto"
    local rng = crypto.ThreadRng
    local t = require "testing"

    t.assert_eq(type(rng:random()), "number")
    t.assert(math.tointeger(rng:gen_range(1, 5)))
    t.assert_false(pcall(rng.gen_range, rng, 1, -1))
  "#
}
