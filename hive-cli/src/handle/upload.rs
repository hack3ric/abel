use crate::Result;
use hive_core::{Hive, Source};
use hyper::{Body, Request, Response};
use multer::{Constraints, Multipart, SizeLimit};

pub async fn run(
  hive: &Hive,
  mut req: Request<Body>,
  name: Option<Box<str>>,
) -> Result<Response<Body>> {
  let name_provided = name.is_some();

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

  let field = multipart
    .next_field()
    .await?
    .ok_or("no source code uploaded")?;
  let name = name
    .or_else(|| {
      field.file_name().map(|mut x| {
        let len = x.len();
        if &x[len - 4..] == ".lua" {
          x = &x[..len - 4];
        }
        slug::slugify(x).into()
      })
    })
    .ok_or("no service name provided")?;

  if !name_provided && hive.get_service(&name).await.is_some() {
    return Err((409, format!("service '{}' already exists", name)))?;
  }

  let source = Source::new_single(field.bytes().await?.as_ref());
  let service = hive.create_service(name, source).await?;
  let service = service.upgrade();

  Ok(Response::new(serde_json::to_string(&service)?.into()))
}
