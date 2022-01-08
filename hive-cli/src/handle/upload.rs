use crate::Result;
use hive_core::{Hive, Source};
use hyper::{Body, Request, Response};
use multer::{Constraints, Multipart, SizeLimit};

pub async fn run(
  hive: &Hive,
  mut req: Request<Body>,
  name: Option<Box<str>>,
) -> Result<Response<Body>> {
  let content_type = req
    .headers()
    .get("Content-Type")
    .ok_or("no Content-Type given")?
    .to_str()
    .or(Err("Content-Type is not valid UTF-8"))?;

  let boundary = multer::parse_boundary(content_type)?;
  let constraints = Constraints::new()
    .allowed_fields(vec!["source"])
    .size_limit(SizeLimit::new().for_field("source", 1024u64.pow(3)));
  let mut multipart = Multipart::with_constraints(req.body_mut(), boundary, constraints);

  let source = multipart
    .next_field()
    .await?
    .ok_or("no source code uploaded")?;
  let name = name
    .or_else(|| source.file_name().map(|x| slug::slugify(x).into()))
    .ok_or("no service name provided")?;
  let source = Source::new_single(source.bytes().await?.as_ref());

  hive.create_service(name, source).await?;

  Ok(Response::new("upload".into()))
}
