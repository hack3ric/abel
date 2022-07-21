use crate::runtime::{Extra, Runtime};
use crate::{AbelState, Result};
use abel_rt::{Executor, SharedTask};
use futures::Future;
use log::error;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RuntimePool {
  name: String,
  executors: Vec<RwLock<Executor<Runtime, Extra>>>,
  state: Arc<AbelState>,
}

impl RuntimePool {
  pub fn new(name: String, size: usize, state: Arc<AbelState>) -> Result<Self> {
    let executors = (0..size)
      .map(|i| {
        let state = state.clone();
        Ok(RwLock::new(Executor::new(
          |tx| Runtime::new(state, tx.clone()),
          format!("{}-{i}", name),
        )))
      })
      .collect::<Result<_>>()?;

    Ok(Self {
      name,
      executors,
      state,
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
        let state = self.state.clone();
        *wl = Executor::new(
          |tx| Runtime::new(state, tx.clone()),
          format!("{}-{i}", self.name),
        );
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
