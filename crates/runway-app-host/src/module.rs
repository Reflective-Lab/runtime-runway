use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;

use crate::context::HostContext;

/// Placeholder type alias for a Tonic-served service.
///
/// In Phase 1 we don't wire gRPC bring-up; modules return an empty `Vec`.
/// `tonic::transport::server::Routes` is the simplest concrete type that
/// compiles cleanly against tonic 0.11 and can be collected into a `Vec`.
pub type TonicService = tonic::service::Routes;

#[async_trait]
pub trait HelmModule: Send + Sync + 'static {
    fn module_id(&self) -> &'static str;

    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()>;

    fn router(self: Arc<Self>) -> Router {
        Router::new()
    }

    fn grpc_services(self: Arc<Self>) -> Vec<TonicService> {
        vec![]
    }
}
