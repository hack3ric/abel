use crate::runtime::{Extra, Runtime};
use crate::Result;
use abel_rt::{mlua, Executor, SharedTask};
use futures::Future;
use log::error;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RuntimePool {
  name: String,
  executors: Vec<RwLock<Executor<Runtime, Extra>>>,
  init: Arc<dyn Fn() -> mlua::Result<Runtime> + Send + Sync + 'static>,
}

impl RuntimePool {
  pub fn new(
    name: String,
    size: usize,
    init: impl Fn() -> mlua::Result<Runtime> + Send + Sync + 'static,
  ) -> Result<Self> {
    let init: Arc<dyn Fn() -> _ + Send + Sync + 'static> = Arc::new(init);

    let executors = (0..size)
      .map(|i| {
        let init = init.clone();
        Ok(RwLock::new(Executor::new(
          // Since `Arc<dyn Fn> does not implement `Fn{,Mut,Once}`, we need to stop clippy
          // from complaining us to wrap it in another closure.
          #[allow(clippy::redundant_closure)]
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
    F: FnOnce(Rc<Runtime>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'a,
    R: Send + 'static,
  {
    let (task, rx) = SharedTask::new(task_fn);

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

    *rx.await.unwrap()
  }
}
