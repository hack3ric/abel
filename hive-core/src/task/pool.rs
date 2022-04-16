use super::executor::Executor;
use super::Task;
use crate::lua::Sandbox;
use crate::Result;
use futures::{Future, FutureExt};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, Mutex, RwLock};

pub struct Pool<T: 'static> {
  name: String,
  executors: Vec<RwLock<Executor<T>>>,
  task_tx: broadcast::Sender<Task<T>>,
  init: Arc<dyn Fn() -> Result<T> + Send + Sync + 'static>,
}

// Since `Arc<dyn Fn> does not implement `Fn{,Mut,Once}`, we need to stop clippy
// from complaining us to wrap it in another closure.
#[allow(clippy::redundant_closure)]
impl Pool<Sandbox> {
  pub fn new(
    name: String,
    size: usize,
    init: impl Fn() -> Result<Sandbox> + Send + Sync + 'static,
  ) -> Result<Self> {
    let init: Arc<dyn Fn() -> _ + Send + Sync + 'static> = Arc::new(init);

    let (task_tx, _) = broadcast::channel(255);
    let executors = (0..size)
      .map(|i| {
        let init = init.clone();
        Ok(RwLock::new(Executor::new(
          task_tx.subscribe(),
          move || init(),
          format!("{}-{i}", name),
        )))
      })
      .collect::<Result<_>>()?;

    Ok(Self {
      name,
      executors,
      init,
      task_tx,
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
        let init = self.init.clone();
        *wl = Executor::new(
          self.task_tx.subscribe(),
          move || init(),
          format!("{}-{i}", self.name),
        );
      }
    }
    let _ = self.task_tx.send(task);
    *rx.await.unwrap().downcast().unwrap()
  }
}
