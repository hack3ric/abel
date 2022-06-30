use super::AnyBox;
use crate::lua::{context, Runtime};
use futures::future::LocalBoxFuture;
use futures::Future;
use mlua::{ExternalError, HookTriggers, RegistryKey};
use pin_project::pin_project;
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

#[pin_project]
pub struct TaskFuture {
  rt: Rc<Runtime>,
  context: Option<RegistryKey>,
  #[pin]
  task: LocalBoxFuture<'static, AnyBox>,
  tx: Option<oneshot::Sender<AnyBox>>,
  cpu_time: Rc<RefCell<Duration>>,
}

impl TaskFuture {
  pub fn new_with_context(
    rt: Rc<Runtime>,
    task_fn: impl FnOnce(Rc<Runtime>) -> LocalBoxFuture<'static, AnyBox>,
    tx: oneshot::Sender<AnyBox>,
  ) -> mlua::Result<Self> {
    let context = context::create(rt.lua())?;
    Ok(Self {
      rt: rt.clone(),
      context: Some(context),
      task: task_fn(rt),
      tx: Some(tx),
      cpu_time: Rc::new(RefCell::new(Duration::new(0, 0))),
    })
  }
}

// TODO: implement CPU timeout
impl Future for TaskFuture {
  type Output = mlua::Result<()>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let this = self.project();

    context::set_current(this.rt.lua(), this.context.as_ref())?;

    let hook_triggers = HookTriggers::every_nth_instruction(1048576);
    this.rt.lua().set_hook(hook_triggers, {
      let t1 = RefCell::new(Instant::now());
      let cpu_time = this.cpu_time.clone();
      move |_lua, _| {
        let t2 = Instant::now();
        let dur = t2.duration_since(*t1.borrow());
        *cpu_time.borrow_mut() += dur;

        if *cpu_time.borrow() >= Duration::from_secs(1) {
          Err("timeout".to_lua_err())
        } else {
          *t1.borrow_mut() = t2;
          Ok(())
        }
      }
    })?;

    let poll = this.task.poll(cx);
    this.rt.lua().remove_hook();

    match poll {
      Poll::Ready(result) => {
        if let Some(tx) = this.tx.take() {
          let _ = tx.send(result);
          if let Some(context) = this.context.take() {
            context::destroy(this.rt.lua(), context)?;
          }
        }
        Poll::Ready(Ok(()))
      }
      Poll::Pending => {
        context::set_current(this.rt.lua(), None)?;
        Poll::Pending
      }
    }
  }
}
