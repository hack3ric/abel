mod executor;
mod pool;

pub use pool::SandboxPool;

use futures::future::LocalBoxFuture;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use crate::lua::Sandbox;

type AnyBox = Box<dyn Any + Send>;
type TaskFn = Box<(dyn FnOnce(Rc<Sandbox>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;
type Task = Arc<Mutex<Option<(TaskFn, oneshot::Sender<AnyBox>)>>>;
