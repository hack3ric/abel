mod executor;
mod pool;
mod task_future;

pub use pool::SandboxPool;

use crate::lua::Sandbox;
use futures::future::LocalBoxFuture;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

type AnyBox = Box<dyn Any + Send>;
type TaskFn = Box<(dyn FnOnce(Rc<Sandbox>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;
type Task = Arc<Mutex<Option<(TaskFn, oneshot::Sender<AnyBox>)>>>;
