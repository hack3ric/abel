use crate::permission::PermissionSet;
use mlua::UserData;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PermissionBridge(pub(crate) Arc<PermissionSet>);

impl UserData for PermissionBridge {
  // TODO: Permission API
}
