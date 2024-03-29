mod context;
mod executor;
mod pool;
mod task_future;

pub use context::{close_value, TaskContext};
pub use executor::Executor;
pub use pool::Pool;
pub use task_future::TimeoutError;

use crate::runtime::Runtime;
use futures::future::LocalBoxFuture;
use futures::{Future, FutureExt, TryFutureExt};
use mlua::Lua;
use parking_lot::Mutex;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::RecvError;

type AnyBox = Box<dyn Any + Send>;
type TaskFn = Box<(dyn FnOnce(Rc<Runtime>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;

pub struct SharedTask(Arc<Mutex<Option<OwnedTask>>>);

impl SharedTask {
  pub fn new<'a, F, Fut>(
    init_cpu_time: Arc<Mutex<Duration>>,
    task_fn: F,
  ) -> (
    Self,
    impl Future<Output = Result<Box<Fut::Output>, RecvError>> + Send + 'a,
  )
  where
    F: FnOnce(Rc<Runtime>) -> Fut + Send + 'static,
    Fut: Future + 'a,
    Fut::Output: Send + 'static,
  {
    let (task, rx) = OwnedTask::new(init_cpu_time, task_fn);
    let task = Self(Arc::new(Mutex::new(Some(task))));
    (task, rx)
  }

  pub(crate) fn take(&self, lua: &Lua) -> mlua::Result<Option<LocalTask>> {
    (self.0)
      .try_lock()
      .and_then(|mut x| x.take())
      .map(|x| x.into_local(lua))
      .transpose()
  }
}

impl Clone for SharedTask {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

pub struct OwnedTask {
  task_fn: TaskFn,
  tx: oneshot::Sender<AnyBox>,
  init_cpu_time: Arc<Mutex<Duration>>,
}

impl OwnedTask {
  pub fn new<'a, F, Fut>(
    cpu_time: Arc<Mutex<Duration>>,
    task_fn: F,
  ) -> (
    Self,
    impl Future<Output = Result<Box<Fut::Output>, RecvError>> + Send + 'a,
  )
  where
    F: FnOnce(Rc<Runtime>) -> Fut + Send + 'static,
    Fut: Future + 'a,
    Fut::Output: Send + 'static,
  {
    let task_fn = Box::new(|t| async move { Box::new(task_fn(t).await) as AnyBox }.boxed_local());
    let (tx, rx) = oneshot::channel();
    let task = OwnedTask {
      task_fn,
      tx,
      init_cpu_time: cpu_time,
    };
    let rx = rx.map_ok(|x| x.downcast::<Fut::Output>().unwrap());
    (task, rx)
  }

  pub fn into_local(self, lua: &Lua) -> mlua::Result<LocalTask> {
    let OwnedTask {
      task_fn,
      tx,
      init_cpu_time,
    } = self;
    let mut context = TaskContext::new_with_close_table(lua)?;
    context.cpu_time = init_cpu_time;
    let task = LocalTask {
      task_fn,
      tx,
      context,
    };
    Ok(task)
  }
}

pub struct LocalTask {
  task_fn: TaskFn,
  tx: oneshot::Sender<AnyBox>,
  context: TaskContext,
}

impl LocalTask {
  pub fn new<'a, F, Fut>(
    context: TaskContext,
    task_fn: F,
  ) -> (
    Self,
    impl Future<Output = Result<Box<Fut::Output>, RecvError>> + Send + 'a,
  )
  where
    F: FnOnce(Rc<Runtime>) -> Fut + Send + 'static,
    Fut: Future + 'a,
    Fut::Output: Send + 'static,
  {
    let task_fn = Box::new(|t| async move { Box::new(task_fn(t).await) as AnyBox }.boxed_local());
    let (tx, rx) = oneshot::channel();
    let task = Self {
      task_fn,
      tx,
      context,
    };
    let rx = rx.map_ok(|x| x.downcast::<Fut::Output>().unwrap());
    (task, rx)
  }
}

pub enum Task {
  Shared(SharedTask),
  Owned(OwnedTask),
}

impl Task {
  pub(crate) fn take(self, lua: &Lua) -> mlua::Result<Option<LocalTask>> {
    match self {
      Self::Shared(x) => x.take(lua),
      Self::Owned(x) => x.into_local(lua).map(Some),
    }
  }
}

impl From<SharedTask> for Task {
  fn from(x: SharedTask) -> Self {
    Self::Shared(x)
  }
}

impl From<OwnedTask> for Task {
  fn from(x: OwnedTask) -> Self {
    Self::Owned(x)
  }
}
