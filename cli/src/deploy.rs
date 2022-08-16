use crate::server::types::HttpUploadResponse;
use crate::server::upload::UploadMode;
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
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use uuid::Uuid;

pub async fn deploy(
  server: Option<Uri>,
  auth_token: Option<Uuid>,
  path: PathBuf,
  mode: UploadMode,
) -> anyhow::Result<()> {
  let path = fs::canonicalize(path).await?;
  let server = server.map(Ok).unwrap_or_else(|| {
    var("ABEL_SERVER")
      .context("you need to specify either the env ABEL_SERVER or the argument --server")?
      .parse()
      .context("failed to parse env ABEL_SERVER")
  })?;
  let name = path.file_stem().context("no filename found")?;
  let name = name.to_str().context("filename contains non-UTF-8 bytes")?;
  let server = format!("{server}/services/{name}?mode={mode}");

  let auth_token = auth_token
    .map(|x| Ok(Some(x)))
    .unwrap_or_else(|| {
      std::env::var_os("ABEL_AUTH_TOKEN")
        .map(|x| {
          x.to_str()
            .context("failed to parse ABEL_AUTH_TOKEN as UTF-8")?
            .parse()
            .context("failed to parse env ABEL_AUTH_TOKEN into UUID")
        })
        .transpose()
    })?
    .map(|x| {
      let mut x = HeaderValue::try_from(format!("Abel {x}"))?;
      x.set_sensitive(true);
      anyhow::Ok(x)
    })
    .transpose()?;

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

  let mut builder = Client::new().put(server);
  if let Some(x) = auth_token {
    builder = builder.header("authorization", x);
  }
  let resp = builder.multipart(form).send().await?;

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

  if !resp.errors.is_empty() {
    println!("Errors:");
    if resp.errors.start.is_some() {
      println!(
        "  - Start: {}",
        resp
          .errors
          .start
          .as_deref()
          .map(|x| Cow::Owned(x.replace('\n', "\n    ")))
          .unwrap_or(Cow::Borrowed("None"))
      );
    }
    if resp.errors.stop.is_some() {
      println!(
        "  - Stop: {}",
        resp
          .errors
          .stop
          .as_deref()
          .map(|x| Cow::Owned(x.replace('\n', "\n    ")))
          .unwrap_or(Cow::Borrowed("None"))
      );
    }
  }

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
