use anyhow::{bail, Context};
use hyper::http::HeaderValue;
use owo_colors::OwoColorize;
use reqwest::multipart::{Form, Part};
use reqwest::{Body, Client};
use serde::Deserialize;
use std::ffi::OsStr;
use std::path::PathBuf;
use tokio::fs::{self, File};

pub async fn deploy(path: PathBuf) -> anyhow::Result<()> {
  let dest = std::env::var("ABEL_SERVER").context("failed to get env ABEL_SERVER")?;
  let name = path.file_stem().context("no filename found")?;
  let name = name.to_str().context("filename contains non-UTF-8 bytes")?;
  let dest = format!("{dest}/services/{name}?mode=cold");

  let auth_token = std::env::var("ABEL_AUTH_TOKEN").context("failed to get env ABEL_AUTH_TOKEN")?;
  let mut auth_token = HeaderValue::try_from(format!("Abel {auth_token}"))?;
  auth_token.set_sensitive(true);

  let metadata = fs::metadata(&path).await?;
  let form = if metadata.is_dir() {
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
    .put(dest)
    .header("authorization", auth_token)
    .multipart(form)
    .send()
    .await?;

  #[derive(Deserialize)]
  struct JsonError {
    error: String,
    detail: Option<serde_json::Value>,
  }

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

  println!("{}", String::from_utf8_lossy(&resp.bytes().await?));
  // resp.error_for_status_ref()?;

  Ok(())
}
