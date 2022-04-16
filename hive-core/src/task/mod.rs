mod executor;
mod pool;

pub use pool::Pool;

use futures::future::LocalBoxFuture;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

type AnyBox = Box<dyn Any + Send>;
type TaskFn<T> = Box<(dyn FnOnce(Rc<T>) -> LocalBoxFuture<'static, AnyBox> + Send + 'static)>;
type Task<T> = Arc<Mutex<Option<(TaskFn<T>, oneshot::Sender<AnyBox>)>>>;
