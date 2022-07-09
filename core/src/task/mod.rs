mod executor;
mod pool;
mod task_future;

pub use pool::RuntimePool;

use crate::runtime::Runtime;
use futures::future::LocalBoxFuture;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

type AnyBox = Box<dyn Any + Send>;
type TaskFn = Box<(dyn FnOnce(Rc<Runtime>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;
type Task = Arc<Mutex<Option<(TaskFn, oneshot::Sender<AnyBox>)>>>;
