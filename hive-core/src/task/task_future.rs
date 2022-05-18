use super::AnyBox;
use crate::lua::{context, Sandbox};
use futures::future::LocalBoxFuture;
use futures::Future;
use mlua::{RegistryKey, Table};
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use tokio::sync::oneshot;

pub struct TaskFuture {
  sandbox: Rc<Sandbox>,
  context: Option<RegistryKey>,
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

  fn get_context_table(&self) -> mlua::Result<Option<Table>> {
    (self.context)
      .as_ref()
      .map(|x| self.sandbox.lua.registry_value(x))
      .transpose()
  }
}

impl Future for TaskFuture {
  type Output = mlua::Result<()>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
    let tx = if let Some(tx) = self.tx.take() {
      tx
    } else {
      return Poll::Ready(Ok(()));
    };

    let context = self.get_context_table()?;
    context::set_current(&self.sandbox.lua, context.clone())?;
    drop(context);

    match Pin::new(&mut self.task).poll(cx) {
      Poll::Ready(result) => {
        let _ = tx.send(result);
        if let Some(context) = self.get_context_table()? {
          context::destroy(&self.sandbox.lua, context)?;
        }
        Poll::Ready(Ok(()))
      }
      Poll::Pending => {
        context::set_current(&self.sandbox.lua, None)?;
        self.tx = Some(tx);
        Poll::Pending
      }
    }
  }
}
