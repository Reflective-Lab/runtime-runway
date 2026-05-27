use std::sync::Arc;

use runway_storage::StorageKit;

use crate::AppExecutionPacket;
use crate::realtime::EventHubHandle;

#[derive(Clone)]
pub struct HostContext {
    pub packet: Arc<AppExecutionPacket>,
    pub storage: StorageKit,
    pub realtime: EventHubHandle,
}
