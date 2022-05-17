use crate::lua::{context, Sandbox};
use futures::future::LocalBoxFuture;
use futures::Future;
use mlua::{RegistryKey, Table};
use std::any::Any;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use tokio::sync::oneshot;

pub struct TaskFuture {
  sandbox: Rc<Sandbox>,
  context: Option<RegistryKey>,
  task: LocalBoxFuture<'static, Box<dyn Any + Send>>,
  tx: Option<oneshot::Sender<Box<dyn Any + Send>>>,
}

impl TaskFuture {
  pub fn new(
    sandbox: Rc<Sandbox>,
    task_fn: impl FnOnce(Rc<Sandbox>) -> LocalBoxFuture<'static, Box<dyn Any + Send>>,
    tx: oneshot::Sender<Box<dyn Any + Send>>,
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
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
    let tx = if let Some(tx) = self.tx.take() {
      tx
    } else {
      return Poll::Ready(());
    };

    let context = self.get_context_table().unwrap();
    context::set_current(&self.sandbox.lua, context.clone()).unwrap();
    drop(context);

    match Pin::new(&mut self.task).poll(cx) {
      Poll::Ready(result) => {
        let _ = tx.send(result);
        if let Some(context) = self.get_context_table().unwrap() {
          context::destroy(&self.sandbox.lua, context).unwrap();
        }
        Poll::Ready(())
      }
      Poll::Pending => {
        context::set_current(&self.sandbox.lua, None).unwrap();
        self.tx = Some(tx);
        Poll::Pending
      }
    }
  }
}
