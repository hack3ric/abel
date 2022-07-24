mod executor;
mod task_future;

pub use executor::Executor;
pub use task_future::TimeoutError;

use crate::lua::context::TaskContext;
use futures::future::LocalBoxFuture;
use futures::{Future, FutureExt, TryFutureExt};
use mlua::Lua;
use parking_lot::Mutex;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::oneshot::{self};

type AnyBox = Box<dyn Any + Send>;
type TaskFn<T> = Box<(dyn FnOnce(Rc<T>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;

pub struct SharedTask<T: 'static>(Arc<Mutex<Option<OwnedTask<T>>>>);

impl<T> SharedTask<T> {
  pub fn new<'a, F, Fut>(
    init_cpu_time: Arc<Mutex<Duration>>,
    task_fn: F,
  ) -> (
    Self,
    impl Future<Output = Result<Box<Fut::Output>, RecvError>> + Send + 'a,
  )
  where
    F: FnOnce(Rc<T>) -> Fut + Send + 'static,
    Fut: Future + 'a,
    Fut::Output: Send + 'static,
  {
    let (task, rx) = OwnedTask::new(init_cpu_time, task_fn);
    let task = Self(Arc::new(Mutex::new(Some(task))));
    (task, rx)
  }

  pub(crate) fn take(&self, lua: &Lua) -> mlua::Result<Option<LocalTask<T>>> {
    (self.0)
      .try_lock()
      .and_then(|mut x| x.take())
      .map(|x| x.into_local(lua))
      .transpose()
  }
}

impl<T> Clone for SharedTask<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

pub struct OwnedTask<T: 'static> {
  task_fn: TaskFn<T>,
  tx: oneshot::Sender<AnyBox>,
  init_cpu_time: Arc<Mutex<Duration>>,
}

impl<T> OwnedTask<T> {
  pub fn new<'a, F, Fut>(
    cpu_time: Arc<Mutex<Duration>>,
    task_fn: F,
  ) -> (
    Self,
    impl Future<Output = Result<Box<Fut::Output>, RecvError>> + Send + 'a,
  )
  where
    F: FnOnce(Rc<T>) -> Fut + Send + 'static,
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

  pub fn into_local(self, lua: &Lua) -> mlua::Result<LocalTask<T>> {
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

pub struct LocalTask<T: 'static> {
  task_fn: TaskFn<T>,
  tx: oneshot::Sender<AnyBox>,
  context: TaskContext,
}

impl<T> LocalTask<T> {
  pub fn new<'a, F, Fut>(
    context: TaskContext,
    task_fn: F,
  ) -> (
    Self,
    impl Future<Output = Result<Box<Fut::Output>, RecvError>> + Send + 'a,
  )
  where
    F: FnOnce(Rc<T>) -> Fut + Send + 'static,
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

pub enum Task<T: 'static> {
  Shared(SharedTask<T>),
  Owned(OwnedTask<T>),
}

impl<T> Task<T> {
  pub(crate) fn take(self, lua: &Lua) -> mlua::Result<Option<LocalTask<T>>> {
    match self {
      Self::Shared(x) => x.take(lua),
      Self::Owned(x) => x.into_local(lua).map(Some),
    }
  }
}

impl<T> From<SharedTask<T>> for Task<T> {
  fn from(x: SharedTask<T>) -> Self {
    Self::Shared(x)
  }
}

impl<T> From<OwnedTask<T>> for Task<T> {
  fn from(x: OwnedTask<T>) -> Self {
    Self::Owned(x)
  }
}
