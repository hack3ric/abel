mod error;
mod object_pool;
mod service;
mod source;

use error::HiveResult;

pub struct Hive {}

impl Hive {
  pub fn new() -> HiveResult<Self> { Ok(Self {}) }
}
