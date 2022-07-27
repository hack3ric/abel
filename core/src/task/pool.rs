use crate::runtime::Runtime;
use crate::task::{Executor, SharedTask};
use crate::Result;
use futures::Future;
use log::error;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Pool {
  executors: Vec<RwLock<Executor>>,
  f: Arc<dyn Fn() -> mlua::Result<Runtime> + Send + Sync>,
}

impl Pool {
  pub fn new(
    size: usize,
    f: impl Fn() -> mlua::Result<Runtime> + Send + Sync + 'static,
  ) -> Result<Self> {
    let f = Arc::new(f);
    let executors = (0..size)
      .map(|i| {
        let f = f.clone();
        Ok(RwLock::new(Executor::new(
          move || f(),
          format!("abel-worker-{i}"),
        )))
      })
      .collect::<Result<_>>()?;

    Ok(Self { executors, f })
  }

  pub async fn scope<'a, F, Fut, R>(&self, task_fn: F) -> R
  where
    F: FnOnce(Rc<Runtime>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'a,
    R: Send + 'static,
  {
    let (task, rx) = SharedTask::new(Default::default(), task_fn);

    for (i, e) in self.executors.iter().enumerate() {
      let rl = e.read().await;
      let result = if rl.is_panicked() {
        drop(rl);
        let mut wl = e.write().await;
        // let state = self.state.clone();
        let f = self.f.clone();
        *wl = Executor::new(move || f(), format!("abel-worker-{i}"));
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
