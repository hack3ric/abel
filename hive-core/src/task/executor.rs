use crate::lua::Sandbox;
use crate::Result;
use futures::future::{select, Either, LocalBoxFuture};
use futures::stream::FuturesUnordered;
use futures::{pin_mut, FutureExt, Stream};
use log::info;
use std::any::Any;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::Instant;

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

struct PanicNotifier(Arc<AtomicBool>);

impl Drop for PanicNotifier {
  fn drop(&mut self) {
    if std::thread::panicking() {
      self.0.store(true, Ordering::Release)
    }
  }
}

type AnyBox = Box<dyn Any + Send>;
type Task<T> = Arc<Mutex<Option<(TaskFn<T>, oneshot::Sender<AnyBox>)>>>;
type TaskFn<T> = Box<(dyn FnOnce(Rc<T>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;

// TODO: use broadcast?
pub struct Executor<T: 'static> {
  pub task_count: Arc<AtomicU32>,
  task_tx: mpsc::UnboundedSender<Task<T>>,
  panicked: Arc<AtomicBool>,
}

impl Executor<Sandbox> {
  pub fn new(f: impl FnOnce() -> Result<Sandbox> + Send + 'static, name: String) -> Self {
    let task_count = Arc::new(AtomicU32::new(0));
    let (task_tx, mut task_rx) = mpsc::unbounded_channel::<Task<_>>();
    let panicked = Arc::new(AtomicBool::new(false));
    let panic_notifier = PanicNotifier(panicked.clone());

    let rt = Handle::current();
    let task_count2 = task_count.clone();
    std::thread::Builder::new()
      .name(name)
      .spawn(move || {
        let _panic_notifier = panic_notifier;
        rt.block_on(async move {
          let obj = f().unwrap();
          let mut tasks = FuturesUnordered::<LocalBoxFuture<()>>::new();
          let (waker_tx, mut waker_rx) = mpsc::unbounded_channel();
          let mut waker = MyWaker::from_tx(waker_tx.clone());
          let obj = Rc::new(obj);

          let dur = Duration::from_secs(600);
          let mut clean_interval = tokio::time::interval_at(Instant::now() + dur, dur);

          loop {
            let waker_recv = waker_rx.recv();
            let new_task_recv = task_rx.recv();
            let clean = clean_interval.tick();
            pin_mut!(waker_recv, new_task_recv, clean);

            match select(select(waker_recv, clean), new_task_recv).await {
              Either::Left((Either::Left(..), _)) => {
                waker = MyWaker::from_tx(waker_tx.clone());
                let tasks = Pin::new(&mut tasks);
                let mut context = Context::from_waker(&waker);
                if let Poll::Ready(Some(_)) = tasks.poll_next(&mut context) {
                  waker.wake_by_ref();
                }
              }
              Either::Left((Either::Right(..), _)) => {
                // TODO: better cleaning trigger
                let count = obj.clean_loaded().await;
                if count > 0 {
                  info!("successfully cleaned {count} dropped services");
                }
              }
              Either::Right((Some(msg), _)) => {
                if let Some((task, tx)) = msg.lock().await.take() {
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
                  ));
                  waker.wake_by_ref();
                }
              }
              Either::Right((None, _)) => break,
            }
          }
        })
      })
      .unwrap();

    Self {
      task_count,
      task_tx,
      panicked,
    }
  }

  pub(crate) fn push<R: Send + 'static>(&self, task: Task<Sandbox>) {
    let _ = self.task_tx.send(task);
  }

  pub fn is_panicked(&self) -> bool {
    self.panicked.load(Ordering::Acquire)
  }
}
