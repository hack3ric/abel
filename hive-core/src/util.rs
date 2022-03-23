use serde::{Serialize, Serializer};
use std::sync::Arc;

pub fn serialize_arc<S: Serializer>(arc: &Arc<impl Serialize>, ser: S) -> Result<S::Ok, S::Error> {
  arc.as_ref().serialize(ser)
}
