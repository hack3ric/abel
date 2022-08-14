use crate::server::types::HttpUploadResponse;
use crate::server::JsonError;
use anyhow::{bail, Context};
use hyper::http::HeaderValue;
use hyper::Uri;
use log::debug;
use owo_colors::OwoColorize;
use reqwest::multipart::{Form, Part};
use reqwest::{Body, Client};
use std::borrow::Cow;
use std::env::var;
use std::ffi::OsStr;
use std::path::{PathBuf, Path};
use tokio::fs::{self, File};
use uuid::Uuid;

pub async fn deploy(
  server: Option<Uri>,
  auth_token: Option<Uuid>,
  path: PathBuf,
) -> anyhow::Result<()> {
  let server = server.map(Ok).unwrap_or_else(|| {
    var("ABEL_SERVER")
      .context("you need to specify either the env ABEL_SERVER or the argument --server")?
      .parse()
      .context("failed to parse env ABEL_SERVER")
  })?;
  let name = path.file_stem().context("no filename found")?;
  let name = name.to_str().context("filename contains non-UTF-8 bytes")?;
  let server = format!("{server}/services/{name}?mode=cold");

  let auth_token = auth_token.map(Ok).unwrap_or_else(|| {
    var("ABEL_AUTH_TOKEN")
      .context("you need to specify either the env ABEL_AUTH_TOKEN or the argument --auth-token")?
      .parse()
      .context("failed to parse env ABEL_AUTH_TOKEN")
  })?;
  let mut auth_token = HeaderValue::try_from(format!("Abel {auth_token}"))?;
  auth_token.set_sensitive(true);

  let metadata = fs::metadata(&path).await?;
  let form = if metadata.is_dir() {
    check_folder(&path)?;
    let asar_stream = hive_asar::pack_dir_into_stream(path)
      .await
      .context("failed to pack directory into asar")?;
    Form::new().part("multi", Part::stream(Body::wrap_stream(asar_stream)))
  } else {
    let kind = match path.extension().and_then(OsStr::to_str) {
      Some("asar") => "multi",
      Some("lua") => "single",
      _ => {
        println!(
          "{} unknown file extension, assuming as Lua file",
          "warn:".yellow().bold(),
        );
        "single"
      }
    };
    let file = File::open(&path).await?;
    Form::new().part(kind, Part::stream_with_length(file, metadata.len()))
  };

  let resp = Client::new()
    .put(server)
    .header("authorization", auth_token)
    .multipart(form)
    .send()
    .await?;

  let status = resp.status();
  if status.is_client_error() || status.is_server_error() {
    let JsonError { error, detail } = resp
      .json()
      .await
      .context("failed to read JSON from response body")?;
    if let Some(detail) = detail {
      let detail = serde_json::to_string_pretty(&detail)?;
      bail!("server responded with error '{error}' ({status})\n\nDetail: {detail}");
    } else {
      bail!("server responded with error '{error}' ({status})")
    }
  }

  let resp: HttpUploadResponse = resp.json().await?;
  let prefix = resp
    .replaced_service
    .is_some()
    .then_some("Updated")
    .unwrap_or("Created");
  let suffix = resp
    .errors
    .is_empty()
    .then_some("")
    .unwrap_or(" with error");
  println!(
    "{prefix} service '{}' ({}){suffix}",
    resp.new_service.service.name(),
    resp.new_service.service.uuid()
  );
  println!("Errors:");
  println!(
    "  - Start: {}",
    resp
      .errors
      .start
      .as_deref()
      .map(|x| Cow::Owned(x.replace('\n', "\n    ")))
      .unwrap_or(Cow::Borrowed("None"))
  );
  println!(
    "  - Stop: {}",
    resp
      .errors
      .stop
      .as_deref()
      .map(|x| Cow::Owned(x.replace('\n', "\n    ")))
      .unwrap_or(Cow::Borrowed("None"))
  );

  debug!("Response: {resp:#?}");

  Ok(())
}

fn check_folder(path: &Path) -> anyhow::Result<()> {
  let main_lua_path = path.join("main.lua");
  if main_lua_path.exists() {
    bail!("main.lua not found in {}", path.display());
  }
  Ok(())
}
