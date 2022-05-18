use super::executor::Executor;
use crate::lua::Sandbox;
use crate::Result;
use futures::{Future, FutureExt};
use log::error;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

pub struct SandboxPool {
  name: String,
  executors: Vec<RwLock<Executor>>,
  init: Arc<dyn Fn() -> Result<Sandbox> + Send + Sync + 'static>,
}

// Since `Arc<dyn Fn> does not implement `Fn{,Mut,Once}`, we need to stop clippy
// from complaining us to wrap it in another closure.
#[allow(clippy::redundant_closure)]
impl SandboxPool {
  pub fn new(
    name: String,
    size: usize,
    init: impl Fn() -> Result<Sandbox> + Send + Sync + 'static,
  ) -> Result<Self> {
    let init: Arc<dyn Fn() -> _ + Send + Sync + 'static> = Arc::new(init);

    let executors = (0..size)
      .map(|i| {
        let init = init.clone();
        Ok(RwLock::new(Executor::new(
          move || init(),
          format!("{}-{i}", name),
        )))
      })
      .collect::<Result<_>>()?;

    Ok(Self {
      name,
      executors,
      init,
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
      let result = if rl.is_panicked() {
        drop(rl);
        let mut wl = e.write().await;
        let init = self.init.clone();
        *wl = Executor::new(move || init(), format!("{}-{i}", self.name));
        wl.send(task.clone()).await
      } else {
        rl.send(task.clone()).await
      };
      if result.is_err() {
        error!("task send failed");
      }
    }

    *rx.await.unwrap().downcast().unwrap()
  }
}
