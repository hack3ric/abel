use super::executor::Executor;
use crate::Result;
use futures::{Future, FutureExt};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

pub struct Pool<T: Send + 'static> {
  executors: Vec<RwLock<Executor<T>>>,
  init: Box<dyn Fn() -> Result<T> + Send + Sync + 'static>,
}

impl<T: Send + 'static> Pool<T> {
  pub fn new(
    name: &str,
    size: usize,
    init: impl Fn() -> Result<T> + Send + Sync + 'static,
  ) -> Result<Self> {
    let executors = (0..size)
      .map(|i| {
        Ok(RwLock::new(Executor::new(
          init()?,
          name.to_string() + "-" + &i.to_string(),
        )))
      })
      .collect::<Result<_>>()?;
    Ok(Self {
      executors,
      init: Box::new(init),
    })
  }

  pub async fn scope<'a, F2, Fut, R>(&self, task_fn: F2) -> R
  where
    F2: FnOnce(Rc<T>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'a,
    R: Send + 'static,
  {
    let wrapped_task_fn =
      Box::new(|t| async move { Box::new(task_fn(t).await) as Box<dyn Any + Send> }.boxed_local())
        as Box<_>;
    let (tx, rx) = oneshot::channel();
    let task = Arc::new(Mutex::new(Some((wrapped_task_fn, tx))));

    // TODO: if the thread isn't running (panicked), create a new `Executor` and
    // replace it
    for e in self.executors.iter() {
      let rl = e.read().await;
      if rl.is_panicked() {
        drop(rl);
        let mut wl = e.write().await;
        *wl = Executor::new((self.init)().unwrap(), "name".to_string());
      } else {
        rl.push::<R>(task.clone());
      }
    }

    *rx.await.unwrap().downcast().unwrap()
  }
}
