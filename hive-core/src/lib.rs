#![feature(backtrace)]
#![allow(unused)]
#![warn(unused_imports)]

mod error;
mod lua;
mod object_pool;
mod service;
mod source;

pub use error::{Error, HiveResult};

use lua::Sandbox;
use object_pool::Pool;

pub struct Hive {
  sandbox_pool: Pool<Sandbox>,
}

impl Hive {
  pub fn new() -> HiveResult<Self> {
    Ok(Self {
      sandbox_pool: Pool::with_capacity(8, Sandbox::new)?,
    })
  }
}
