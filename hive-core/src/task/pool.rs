use super::executor::Executor;
use crate::lua::Sandbox;
use crate::Result;
use futures::{Future, FutureExt};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

pub struct Pool<T: Send + 'static> {
  name: String,
  executors: Vec<RwLock<Executor<T>>>,
  init: Box<dyn Fn() -> Result<T> + Send + Sync + 'static>,
}

impl Pool<Sandbox> {
  pub fn new(
    name: String,
    size: usize,
    init: impl Fn() -> Result<Sandbox> + Send + Sync + 'static,
  ) -> Result<Self> {
    let executors = (0..size)
      .map(|i| Ok(RwLock::new(Executor::new(init()?, format!("{}-{i}", name)))))
      .collect::<Result<_>>()?;
    Ok(Self {
      name,
      executors,
      init: Box::new(init),
    })
  }

  pub async fn scope<'a, F, Fut, R>(&self, task_fn: F) -> R
  where
    F: FnOnce(Rc<Sandbox>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'a,
    R: Send + 'static,
  {
    let wrapped_task_fn =
      Box::new(|t| async move { Box::new(task_fn(t).await) as Box<dyn Any + Send> }.boxed_local())
        as Box<_>;
    let (tx, rx) = oneshot::channel();
    let task = Arc::new(Mutex::new(Some((wrapped_task_fn, tx))));

    for (i, e) in self.executors.iter().enumerate() {
      let rl = e.read().await;
      if rl.is_panicked() {
        drop(rl);
        let mut wl = e.write().await;
        *wl = Executor::new((self.init)().unwrap(), format!("{}-{i}", self.name));
      } else {
        rl.push::<R>(task.clone());
      }
    }

    *rx.await.unwrap().downcast().unwrap()
  }
}
