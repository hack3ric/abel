use serde::{Serialize, Serializer};
use std::sync::Arc;

/// Helper struct for implementing `Borrow` for `Arc<ServiceImpl>`.
#[derive(Hash, PartialEq, Eq)]
pub struct MyStr(str);

impl MyStr {
  pub fn new(x: &str) -> &Self {
    <&Self>::from(x)
  }
}

impl<'a> From<&'a str> for &'a MyStr {
  fn from(x: &str) -> &MyStr {
    unsafe { &*(x as *const str as *const MyStr) }
  }
}

pub fn serialize_arc<S: Serializer>(arc: &Arc<impl Serialize>, ser: S) -> Result<S::Ok, S::Error> {
  arc.as_ref().serialize(ser)
}
