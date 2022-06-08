use super::AnyBox;
use crate::lua::{context, Sandbox};
use futures::future::LocalBoxFuture;
use futures::Future;
use mlua::{RegistryKey, Table};
use pin_project::pin_project;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use tokio::sync::oneshot;

#[pin_project]
pub struct TaskFuture {
  sandbox: Rc<Sandbox>,
  context: Option<RegistryKey>,
  #[pin]
  task: LocalBoxFuture<'static, AnyBox>,
  tx: Option<oneshot::Sender<AnyBox>>,
}

impl TaskFuture {
  pub fn new_with_context(
    sandbox: Rc<Sandbox>,
    task_fn: impl FnOnce(Rc<Sandbox>) -> LocalBoxFuture<'static, AnyBox>,
    tx: oneshot::Sender<AnyBox>,
  ) -> mlua::Result<Self> {
    let context = (sandbox.lua).create_registry_value(sandbox.lua.create_table().unwrap())?;
    Ok(Self {
      sandbox: sandbox.clone(),
      context: Some(context),
      task: task_fn(sandbox),
      tx: Some(tx),
    })
  }
}

fn get_context_table<'lua>(
  sandbox: &'lua Rc<Sandbox>,
  context: &Option<RegistryKey>,
) -> mlua::Result<Option<Table<'lua>>> {
  context
    .as_ref()
    .map(|x| sandbox.lua.registry_value(x))
    .transpose()
}

impl Future for TaskFuture {
  type Output = mlua::Result<()>;

  fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
    let this = self.project();
    let tx = if let Some(tx) = this.tx.take() {
      tx
    } else {
      return Poll::Ready(Ok(()));
    };

    let context = get_context_table(this.sandbox, this.context)?;
    context::set_current(&this.sandbox.lua, context.clone())?;
    drop(context);

    match this.task.poll(cx) {
      Poll::Ready(result) => {
        let _ = tx.send(result);
        if let Some(context) = get_context_table(this.sandbox, this.context)? {
          context::destroy(&this.sandbox.lua, context)?;
        }
        Poll::Ready(Ok(()))
      }
      Poll::Pending => {
        context::set_current(&this.sandbox.lua, None)?;
        *this.tx = Some(tx);
        Poll::Pending
      }
    }
  }
}
