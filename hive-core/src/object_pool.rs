use crate::error::HiveResult;
use futures::Future;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::{spawn_blocking, JoinError};

pub struct Pool<T: Debug + Send + 'static> {
  inner: Arc<Mutex<Inner<T>>>,
  tx: mpsc::UnboundedSender<T>,
}

impl<T: Debug + Send + 'static> Clone for Pool<T> {
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      tx: self.tx.clone(),
    }
  }
}

struct Inner<T: Debug + Send + 'static> {
  available: Vec<T>,
  fetch_queue: VecDeque<oneshot::Sender<T>>,
  occupied: u32,
}

impl<T: Debug + Send + 'static> Pool<T> {
  pub fn with_capacity(
    capacity: usize,
    constructor: impl Fn() -> HiveResult<T>,
  ) -> HiveResult<Self> {
    let (tx, mut rx) = mpsc::unbounded_channel::<T>();
    let mut inner = Inner {
      available: Vec::with_capacity(capacity),
      fetch_queue: VecDeque::new(),
      occupied: 0,
    };
    for _ in 0..capacity {
      inner.available.push(constructor()?)
    }
    let inner = Arc::new(Mutex::new(inner));
    let inner2 = inner.clone();
    tokio::spawn(async move {
      while let Some(obj) = rx.recv().await {
        // received a returned object. If there are some fetch request queueing, send it
        // directly to the request, otherwise store this in `available`.
        let mut i = inner2.lock().await;
        if i.fetch_queue.is_empty() {
          i.occupied -= 1;
          i.available.push(obj);
        } else {
          if let Some(req) = i.fetch_queue.pop_back() {
            // in case it happens
            if let Err(obj) = req.send(obj) {
              i.occupied -= 1;
              i.available.push(obj);
            }
          }
        }
      }
    });
    Ok(Pool { inner, tx })
  }

  pub async fn fetch<'a>(&'a self) -> Guarded<T> {
    let mut i = self.inner.lock().await;
    let obj = if let Some(obj) = i.available.pop() {
      i.occupied += 1;
      obj
    } else {
      let (tx, rx) = oneshot::channel::<T>();
      i.fetch_queue.push_front(tx);
      drop(i);
      // XXX: Really should not fail?
      rx.await.unwrap()
    };

    Guarded {
      inner: Some(obj),
      pool: self.clone(),
    }
  }

  pub async fn scope<'a, F, Fut, U>(&self, f: F) -> Result<U, JoinError>
  where
    F: FnOnce(Guarded<T>) -> Fut + Send + 'static,
    Fut: Future<Output = U> + 'a,
    U: Send + 'static,
  {
    let lua = self.fetch().await;
    let rt = Handle::current();
    spawn_blocking(move || rt.block_on(async { f(lua).await })).await
  }
}

pub struct Guarded<T: Debug + Send + 'static> {
  inner: Option<T>,
  pool: Pool<T>,
}

impl<T: Debug + Send + 'static> Drop for Guarded<T> {
  fn drop(&mut self) {
    let obj = self.inner.take().unwrap();
    let tx = self.pool.tx.clone();
    let _ = tx.send(obj);
  }
}

impl<T: Debug + Send + 'static> Deref for Guarded<T> {
  type Target = T;

  fn deref(&self) -> &T {
    self.inner.as_ref().unwrap()
  }
}

impl<T: Debug + Send + 'static> DerefMut for Guarded<T> {
  fn deref_mut(&mut self) -> &mut T {
    self.inner.as_mut().unwrap()
  }
}
