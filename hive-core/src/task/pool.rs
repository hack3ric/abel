use super::executor::Executor;
use crate::Result;
use futures::{Future, FutureExt};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

pub struct Pool<T: Send + 'static> {
  executors: Vec<Executor<T>>,
}

impl<T: Send + 'static> Pool<T> {
  pub fn new(name: &str, size: usize, mut init: impl FnMut() -> Result<T>) -> Result<Self> {
    let executors = (0..size)
      .map(|i| {
        Ok(Executor::new(
          init()?,
          name.to_string() + "-" + &i.to_string(),
        ))
      })
      .collect::<Result<_>>()?;
    Ok(Self { executors })
  }

  pub async fn scope<'a, F, Fut, R>(&self, task_fn: F) -> R
  where
    F: FnOnce(Rc<T>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'a,
    R: Send + 'static,
  {
    let wrapped_task_fn =
      Box::new(|t| async move { Box::new(task_fn(t).await) as Box<dyn Any + Send> }.boxed_local())
        as Box<_>;
    let (tx, rx) = oneshot::channel();
    let task = Arc::new(Mutex::new(Some((wrapped_task_fn, tx))));

    for e in self.executors.iter() {
      e.push::<R>(task.clone());
    }

    *rx.await.unwrap().downcast().unwrap()
  }
}
