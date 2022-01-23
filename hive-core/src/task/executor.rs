use futures::future::{select, Either, LocalBoxFuture};
use futures::stream::FuturesUnordered;
use futures::{pin_mut, Future, FutureExt, Stream};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, Mutex};

struct MyWaker {
  tx: mpsc::UnboundedSender<()>,
  sent: AtomicBool,
}

impl MyWaker {
  fn from_tx(tx: mpsc::UnboundedSender<()>) -> Waker {
    Waker::from(Arc::new(Self {
      tx,
      sent: AtomicBool::new(false),
    }))
  }
}

impl Wake for MyWaker {
  fn wake(self: Arc<Self>) {
    self.wake_by_ref();
  }

  fn wake_by_ref(self: &Arc<Self>) {
    if !self.sent.load(Relaxed) {
      let _ = self.tx.send(());
      self.sent.store(true, Relaxed);
    }
  }
}

type Task<T, R> = Arc<Mutex<Option<TaskFn<T, R>>>>;
type TaskFn<T, R> = Box<(dyn FnOnce(Rc<T>) -> LocalBoxFuture<'static, R> + Send + 'static)>;

pub struct Executor<T: Send + 'static, R: Send + 'static> {
  task_count: Arc<AtomicU32>,
  task_tx: mpsc::UnboundedSender<(Task<T, R>, oneshot::Sender<R>)>,
}

impl<T: Send + 'static, R: Send + 'static> Executor<T, R> {
  pub fn new(obj: T) -> Self {
    let task_count = Arc::new(AtomicU32::new(0));
    let (task_tx, mut task_rx) = mpsc::unbounded_channel::<(Task<T, R>, oneshot::Sender<R>)>();

    let rt = Handle::current();
    let task_count2 = task_count.clone();
    std::thread::spawn(move || {
      rt.block_on(async move {
        let mut tasks = FuturesUnordered::<LocalBoxFuture<()>>::new();
        let (waker_tx, mut waker_rx) = mpsc::unbounded_channel();
        let mut waker = MyWaker::from_tx(waker_tx.clone());
        let obj = Rc::new(obj);

        loop {
          let waker_recv = waker_rx.recv();
          let new_task_recv = task_rx.recv();
          pin_mut!(waker_recv, new_task_recv);

          match select(waker_recv, new_task_recv).await {
            Either::Left(..) => {
              waker = MyWaker::from_tx(waker_tx.clone());
              let tasks = Pin::new(&mut tasks);
              let mut context = Context::from_waker(&waker);
              if let Poll::Ready(Some(_)) = tasks.poll_next(&mut context) {
                waker.wake_by_ref();
              }
            }
            Either::Right((msg, _)) => {
              if let Some((task, tx)) = msg {
                if let Some(task) = task.lock().await.take() {
                  let task_count = task_count2.clone();
                  task_count.fetch_add(1, Ordering::AcqRel);
                  let obj = obj.clone();
                  tasks.push(Box::pin(
                    async move {
                      let result = task(obj).await;
                      let _ = tx.send(result);
                      task_count.fetch_sub(1, Ordering::AcqRel);
                    }
                    .boxed_local(),
                  ) as _);
                }
                waker.wake_by_ref();
              } else {
                // TODO: gracefully shut down
                break;
              }
            }
          }
        }
      })
    });

    Self {
      task_count,
      task_tx,
    }
  }

  pub fn push<F, Fut>(&self, task: F) -> impl Future<Output = R>
  where
    F: FnOnce(Rc<T>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + 'static,
  {
    self._push(|t| task(t).boxed_local())
  }

  fn _push(
    &self,
    task: impl FnOnce(Rc<T>) -> LocalBoxFuture<'static, R> + Send + 'static,
  ) -> impl Future<Output = R> {
    let (tx, rx) = oneshot::channel();
    let _ = self
      .task_tx
      .send((Arc::new(Mutex::new(Some(Box::new(task)))), tx));
    async { rx.await.unwrap() }
  }
}
