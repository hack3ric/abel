use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::Arc;
use super::table::Table;

type ContextStore = Arc<DashMap<Box<str>, Table>>;

static CONTEXT_STORE: Lazy<ContextStore> = Lazy::new(|| Arc::new(DashMap::new()));

pub fn create_context(service_name: Box<str>) -> Table {
  CONTEXT_STORE.entry(service_name).or_insert(Table::new()).clone()
}

pub fn remove_service_contexts(service_name: &str) {
  CONTEXT_STORE.retain(|k, _| k.as_ref() != service_name);
}
