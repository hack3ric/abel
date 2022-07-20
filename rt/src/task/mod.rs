mod executor;
mod task_future;

pub use executor::Executor;

use futures::future::LocalBoxFuture;
use futures::{Future, FutureExt, TryFutureExt};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::oneshot::{self};
use tokio::sync::Mutex;

type AnyBox = Box<dyn Any + Send>;
type TaskFn<T> = Box<(dyn FnOnce(Rc<T>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;

pub struct SharedTask<T: 'static>(Arc<Mutex<Option<OwnedTask<T>>>>);

impl<T> SharedTask<T> {
  pub fn new<'a, F, Fut>(
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
    let (task, rx) = OwnedTask::new(task_fn);
    let task = Self(Arc::new(Mutex::new(Some(task))));
    (task, rx)
  }

  pub(crate) fn take(&self) -> Option<OwnedTask<T>> {
    self.0.try_lock().ok().and_then(|mut x| x.take())
  }
}

impl<T> Clone for SharedTask<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

pub struct OwnedTask<T: 'static>(TaskFn<T>, oneshot::Sender<AnyBox>);

impl<T> OwnedTask<T> {
  pub fn new<'a, F, Fut>(
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
    let wrapped_task_fn =
      Box::new(|t| async move { Box::new(task_fn(t).await) as Box<dyn Any + Send> }.boxed_local())
        as Box<_>;
    let (tx, rx) = oneshot::channel();
    let task = Self(wrapped_task_fn, tx);
    let rx = rx.map_ok(|x| x.downcast::<Fut::Output>().unwrap());
    (task, rx)
  }
}

pub enum Task<T: 'static> {
  Shared(SharedTask<T>),
  Owned(OwnedTask<T>),
}

impl<T> Task<T> {
  pub(crate) fn take(self) -> Option<OwnedTask<T>> {
    match self {
      Self::Shared(x) => x.take(),
      Self::Owned(x) => Some(x),
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
