use hyper::{Body, Response, StatusCode};

pub fn json_response_fn(status: StatusCode, body: String) -> Response<Body> {
  Response::builder()
    .status(status)
    .header("Content-Type", "application/json")
    .body(body.into())
    .unwrap()
}

// TODO: buggy behaviour; maybe replace it after all
#[macro_export]
macro_rules! json_response {
  ($status:expr, $($json:tt)+) => {
    crate::util::json_response_fn($status.try_into().unwrap(), json!($($json)+).to_string())
  };
  ($($json:tt)+) => {
    json_response!(200, $($json)+)
  };
  ($status:expr, $value:expr) => {
    crate::util::json_response_fn($status.try_into().unwrap(), serde_json::to_string($value).unwrap())
  };
  ($value:expr) => {
    json_response!(200, $value)
  };
}
