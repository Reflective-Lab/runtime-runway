use std::sync::Arc;

use runway_storage::StorageKit;

use crate::realtime::EventHubHandle;
use crate::AppExecutionPacket;

#[derive(Clone)]
pub struct HostContext {
    pub packet: Arc<AppExecutionPacket>,
    pub storage: StorageKit,
    pub realtime: EventHubHandle,
}
