use super::{AnyBox, LocalTask};
use crate::lua::context::TaskContext;
use crate::runtime::Runtime;
use futures::future::LocalBoxFuture;
use futures::Future;
use log::error;
use mlua::{self, ExternalError, HookTriggers};
use pin_project::pin_project;
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::oneshot;

#[pin_project]
pub struct TaskFuture {
  rt: Rc<Runtime>,
  context: TaskContext,
  #[pin]
  task: LocalBoxFuture<'static, AnyBox>,
  tx: Option<oneshot::Sender<AnyBox>>,
}

impl TaskFuture {
  pub fn new(
    rt: Rc<Runtime>,
    task_fn: impl FnOnce(Rc<Runtime>) -> LocalBoxFuture<'static, AnyBox>,
    tx: oneshot::Sender<AnyBox>,
    context: TaskContext,
  ) -> Self {
    Self {
      rt: rt.clone(),
      context,
      task: task_fn(rt),
      tx: Some(tx),
    }
  }

  pub fn from_local_task(rt: Rc<Runtime>, task: LocalTask) -> Self {
    #[rustfmt::skip]
    let LocalTask { task_fn, tx, context } = task;
    Self::new(rt, task_fn, tx, context)
  }
}

impl Future for TaskFuture {
  type Output = mlua::Result<()>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let this = self.project();
    let lua = this.rt.lua();

    this.context.set_current(lua);

    let hook_triggers = HookTriggers::every_nth_instruction(1048576);
    lua.set_hook(hook_triggers, {
      let t1 = RefCell::new(Instant::now());
      let cpu_time = this.context.cpu_time.clone();
      move |_lua, _| {
        let mut cpu_time = cpu_time.lock();
        let t2 = Instant::now();
        let dur = t2.duration_since(*t1.borrow());
        *cpu_time += dur;

        if *cpu_time >= Duration::from_secs(1) {
          Err(TimeoutError(()).to_lua_err())
        } else {
          *t1.borrow_mut() = t2;
          Ok(())
        }
      }
    })?;

    let poll = this.task.poll(cx);
    lua.remove_hook();
    let x = TaskContext::remove_current(lua);
    assert_eq!(x.as_ref(), Some(&*this.context));
    drop(x);

    match poll {
      Poll::Ready(result) => {
        if let Some(tx) = this.tx.take() {
          let _ = tx.send(result);
          this.context.try_close(lua)?;
        }
        Poll::Ready(Ok(()))
      }
      Poll::Pending => Poll::Pending,
    }
  }
}

#[derive(Debug, Error)]
#[error("timeout")]
pub struct TimeoutError(pub(crate) ());
