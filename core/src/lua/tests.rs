use super::error::resolve_callback_error;
use super::sandbox::Sandbox;
use crate::source::{Source, SourceVfs};
use async_trait::async_trait;
use std::io::Cursor;
use tempfile::TempDir;
use tokio::io;

pub struct EmptySource;

#[async_trait]
impl SourceVfs for EmptySource {
  type File = Cursor<Vec<u8>>;

  async fn get(&self, _path: &str) -> io::Result<Self::File> {
    Err(io::Error::from_raw_os_error(libc::ENOENT))
  }

  async fn exists(&self, _path: &str) -> io::Result<bool> {
    Ok(false)
  }
}

macro_rules! run_lua_test {
  ($test_name:expr, $code:literal) => {
    async {
      let _ = pretty_env_logger::try_init();
      let sandbox = Sandbox::new()?;
      let local_storage = TempDir::new()?;
      let isolate = sandbox
        .create_isolate($test_name, local_storage.path(), Source::new(EmptySource))
        .await?;
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
          log::error!("{}", error_to_string(&error));
        }
      }
    )*
  };
}

lua_tests! {
  test_json r#"
    local json = require "json"

    assert(json.stringify {} == '{}')
    assert(json.stringify { "foo", "bar" } == '["foo","bar"]')
    assert(json.stringify(nil) == 'null')

    local table = {}
    assert(json.stringify(table) == '{}')
    assert(json.stringify(json.array(table)) == '[]')
    assert(json.stringify(json.undo_array(table)) == '{}')
  "#

  test_http_uri r#"
    local http = require "http"

    assert(uri.scheme == "https")
    assert(uri.host == "test.example.com")
    assert(uri.port == 8080)
    assert(uri.path == "/path")
    assert(uri.query_string == "foo=bar&baz=%20")

    -- Ignores fragment intentionally (see https://github.com/hyperium/hyper/issues/1345)
    assert(tostring(uri) == "https://test.example.com:8080/path?foo=bar&baz=%20")

    local query = uri:query()
    assert(type(query) == "table")
    assert(query.foo == "bar")
    assert(query.baz == " ")
  "#

  test_crypto_random r#"
    local crypto = require "crypto"
    local rng = crypto.thread_rng

    assert(type(rng:random()) == "number")
    assert(math.tointeger(rng:gen_range(1, 5)))
    assert(not pcall(rng.gen_range, rng, 1, -1))
  "#
}
