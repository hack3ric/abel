use hive_core::{Hive, HiveResult, Source};
use hyper::{Request, Body, Response};
use multer::{Constraints, SizeLimit, Multipart};
use std::backtrace::Backtrace;

#[derive(Debug, thiserror::Error)]
enum UploadError {
  #[error(transparent)]
  Hive {
    #[from]
    #[backtrace]
    source: hive_core::Error
  },
  #[error("{source}")]
  Multer {
    #[from]
    source: multer::Error,
    backtrace: Backtrace
  },
}

async fn upload(
  hive: &Hive,
  mut req: Request<Body>,
  name: Option<Box<str>>,
) -> Result<Response<Body>, UploadError> {
  let boundary = multer::parse_boundary(req.headers()["content-type"].to_str().unwrap())?;
  let constraints = Constraints::new()
    .allowed_fields(vec!["source"])
    .size_limit(SizeLimit::new().for_field("source", 1024u64.pow(3)));
  let mut multipart = Multipart::with_constraints(req.body_mut(), boundary, constraints);
  let source = multipart
    .next_field()
    .await
    .unwrap()
    .ok_or_else(|| -> hive_core::Error { unimplemented!("error no code") })?;
  let name = name
    .or_else(|| source.file_name().map(|x| slug::slugify(x).into()))
    .ok_or_else(|| -> hive_core::Error { unimplemented!("error no name") })?;
  let source = Source::new_single(source.bytes().await?.as_ref());

  hive.create_service(name, source).await?;

  Ok(Response::new("upload".into()))
}

pub async fn run(
  hive: &Hive,
  req: Request<Body>,
  name: Option<Box<str>>,
) -> HiveResult<Response<Body>> {
  match upload(hive, req, name).await {
    Ok(x) => Ok(x),
    Err(UploadError::Hive { .. }) => Ok(Response::builder().status(500).body("body".into()).unwrap()),
    Err(UploadError::Multer { backtrace, .. }) => {
      println!("{}", backtrace);
      Ok(Response::builder().status(500).body("body".into()).unwrap())
    }
  }
}
