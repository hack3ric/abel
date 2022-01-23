use super::executor::Executor;
use crate::Result;
use futures::{Future, FutureExt};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Pool<T: Send + 'static> {
  executors: Vec<Executor<T>>,
}

impl<T: Send + 'static> Pool<T> {
  pub fn with_capacity(capacity: usize, init: impl Fn() -> Result<T>) -> Result<Self> {
    let executors = std::iter::repeat_with(|| Ok(Executor::new(init()?)))
      .take(capacity)
      .collect::<Result<_>>()?;
    Ok(Self { executors })
  }

  pub async fn scope<'a, F, Fut, R>(&self, task: F) -> R
  where
    F: FnOnce(Rc<T>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'a,
    R: Send + 'static,
  {
    let x = Arc::new(Mutex::new(Some(Box::new(|t| {
      async move { Box::new(task(t).await) as Box<dyn Any + Send> }.boxed_local()
    }) as Box<_>)));
    self.executors[0].push::<R>(x).await
  }
}
