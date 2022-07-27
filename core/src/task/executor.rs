use super::task_future::TaskFuture;
use super::{LocalTask, Task};
use crate::runtime::Runtime;
use futures::future::select;
use futures::future::Either::*;
use futures::stream::FuturesUnordered;
use futures::{pin_mut, Stream};
use log::{debug, error};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, Ordering};
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
  panicked: Arc<AtomicBool>,
  task_tx: mpsc::Sender<Task>,
  _stop_tx: oneshot::Sender<()>,
}

impl Executor {
  pub fn new(f: impl FnOnce() -> mlua::Result<Runtime> + Send + 'static, name: String) -> Self {
    let panicked = Arc::new(AtomicBool::new(false));
    let panic_notifier = PanicNotifier(panicked.clone());
    let (task_tx, mut task_rx) = mpsc::channel::<Task>(16);
    let (_stop_tx, mut stop_rx) = oneshot::channel();

    let handle = Handle::current();
    std::thread::Builder::new()
      .name(name)
      .spawn(move || {
        let _panic_notifier = panic_notifier;

        handle.block_on(async move {
          let rt = Rc::new(f().unwrap());
          let mut tasks = FuturesUnordered::<TaskFuture>::new();
          let (waker_tx, mut waker_rx) = mpsc::unbounded_channel();
          let mut waker = MyWaker::from_tx(waker_tx.clone());

          rt.lua().set_app_data(Vec::<LocalTask>::new());

          let dur = Duration::from_secs(600);
          let mut clean_interval = tokio::time::interval_at(Instant::now() + dur, dur);

          loop {
            {
              let mut local_tasks = rt.lua().app_data_mut::<Vec<LocalTask>>().unwrap();
              if !local_tasks.is_empty() {
                let iter = local_tasks
                  .drain(..)
                  .map(|task| TaskFuture::from_local_task(rt.clone(), task));
                tasks.extend(iter);
                drop(local_tasks);
                waker_poll(&mut waker, &waker_tx, &mut tasks);
              }
            }

            let stop_rx_mut = Pin::new(&mut stop_rx);
            let waker_recv = waker_rx.recv();
            let clean = clean_interval.tick();
            pin_mut!(waker_recv, clean);

            // SAFETY: `new_task_recv` is never moved
            let mut new_task_recv_ = task_rx.recv();
            let new_task_recv = unsafe { Pin::new_unchecked(&mut new_task_recv_) };

            let select = select(
              select(stop_rx_mut, waker_recv),
              select(clean, new_task_recv),
            );
            match select.await {
              Left((Left(_), _)) => {
                debug!("{} stopping", std::thread::current().name().unwrap());
                break;
              }
              Left((Right(_), _)) => waker_poll(&mut waker, &waker_tx, &mut tasks),
              Right((Left(_), _)) => rt.cleanup(),
              Right((Right((Some(msg), _)), _)) => {
                drop(new_task_recv_);
                if let Some(task) = msg.take(rt.lua()).unwrap() {
                  tasks.push(TaskFuture::from_local_task(rt.clone(), task));
                  while let Ok(task) = task_rx.try_recv() {
                    if let Some(task) = task.take(rt.lua()).unwrap() {
                      tasks.push(TaskFuture::from_local_task(rt.clone(), task));
                    }
                  }
                  waker_poll(&mut waker, &waker_tx, &mut tasks);
                }
              }
              // The new task channel is dropped, stopping the executor.
              Right((Right((None, _)), _)) => break,
            }
          }
        })
      })
      .unwrap();

    Self {
      panicked,
      task_tx,
      _stop_tx,
    }
  }

  pub async fn send(&self, task: impl Into<Task>) -> Result<(), mpsc::error::SendError<Task>> {
    self.task_tx.send(task.into()).await
  }

  pub fn is_panicked(&self) -> bool {
    self.panicked.load(Ordering::Acquire)
  }
}

fn waker_poll(
  waker: &mut Waker,
  waker_tx: &mpsc::UnboundedSender<()>,
  tasks: &mut FuturesUnordered<TaskFuture>,
) {
  *waker = MyWaker::from_tx(waker_tx.clone());
  let mut context = Context::from_waker(waker);
  if let Poll::Ready(Some(result)) = Pin::new(&mut *tasks).poll_next(&mut context) {
    if let Err(error) = result {
      error!("polling task failed: {error}");
    }
    if !tasks.is_empty() {
      waker_poll(waker, waker_tx, tasks);
    }
  }
}
