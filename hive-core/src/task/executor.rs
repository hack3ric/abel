use super::task_future::TaskFuture;
use super::Task;
use crate::lua::Sandbox;
use crate::Result;
use futures::future::{select, Either};
use futures::stream::FuturesUnordered;
use futures::{pin_mut, Stream};
use log::{error, info};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};
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

pub struct Executor {
  pub task_count: Arc<AtomicU32>,
  panicked: Arc<AtomicBool>,
  task_tx: mpsc::Sender<Task>,
  _stop_tx: oneshot::Sender<()>,
}

impl Executor {
  pub fn new(f: impl FnOnce() -> Result<Sandbox> + Send + 'static, name: String) -> Self {
    let task_count = Arc::new(AtomicU32::new(0));
    let panicked = Arc::new(AtomicBool::new(false));
    let panic_notifier = PanicNotifier(panicked.clone());
    let (task_tx, mut task_rx) = mpsc::channel::<Task>(16);
    let (_stop_tx, mut stop_rx) = oneshot::channel();

    let rt = Handle::current();
    let task_count2 = task_count.clone();
    std::thread::Builder::new()
      .name(name)
      .spawn(move || {
        let _panic_notifier = panic_notifier;
        rt.block_on(async move {
          let sandbox = Rc::new(f().unwrap());
          let mut tasks = FuturesUnordered::<TaskFuture>::new();
          let (waker_tx, mut waker_rx) = mpsc::unbounded_channel();
          let mut waker = MyWaker::from_tx(waker_tx.clone());

          let dur = Duration::from_secs(600);
          let mut clean_interval = tokio::time::interval_at(Instant::now() + dur, dur);

          loop {
            let waker_recv = waker_rx.recv();
            let new_task_recv = task_rx.recv();
            let clean = clean_interval.tick();
            let stop_rx_mut = Pin::new(&mut stop_rx);
            pin_mut!(waker_recv, new_task_recv, clean);

            match select(
              select(stop_rx_mut, waker_recv),
              select(clean, new_task_recv),
            )
            .await
            {
              Either::Left((Either::Left(_), _)) => {
                println!("{} stopping", std::thread::current().name().unwrap());
                break;
              }
              Either::Left((Either::Right(_), _)) => {
                waker = MyWaker::from_tx(waker_tx.clone());
                let tasks = Pin::new(&mut tasks);
                let mut context = Context::from_waker(&waker);
                if let Poll::Ready(Some(result)) = tasks.poll_next(&mut context) {
                  if let Err(error) = result {
                    error!("polling task failed: {error}");
                  }
                  waker.wake_by_ref();
                }
              }
              Either::Right((Either::Left(_), _)) => {
                // TODO: better cleaning trigger
                let count = sandbox.clean_loaded().await;
                if count > 0 {
                  info!("successfully cleaned {count} dropped services");
                }
              }
              Either::Right((Either::Right((Some(msg), _)), _)) => {
                if let Some((task_fn, tx)) = msg.try_lock().ok().and_then(|mut x| x.take()) {
                  let task_count = task_count2.clone();
                  task_count.fetch_add(1, Ordering::AcqRel);
                  let task = TaskFuture::new_with_context(sandbox.clone(), task_fn, tx).unwrap();
                  tasks.push(task);
                  waker.wake_by_ref();
                }
              }
              Either::Right((Either::Right((None, _)), _)) => break, // TODO
            }
          }
        })
      })
      .unwrap();

    Self {
      task_count,
      panicked,
      task_tx,
      _stop_tx,
    }
  }

  pub async fn send(&self, task: Task) -> Result<(), mpsc::error::SendError<Task>> {
    self.task_tx.send(task).await
  }

  pub fn is_panicked(&self) -> bool {
    self.panicked.load(Ordering::Acquire)
  }
}
