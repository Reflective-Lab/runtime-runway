use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use tokio::net::TcpListener;

use crate::approvals::{self, ApprovalsState};
use crate::config::HostConfig;
use crate::context::HostContext;
use crate::health;
use crate::module::HelmModule;
use crate::realtime::EventHub;
use crate::sse;
use crate::{AppExecutionPacket, RunwayAppHost};

use runway_storage::StorageKit;

pub struct RunwayAppHostBuilder {
    packet: Arc<AppExecutionPacket>,
    storage: Option<StorageKit>,
    modules: Vec<Arc<dyn HelmModule>>,
    config: Option<HostConfig>,
}

impl RunwayAppHostBuilder {
    pub fn new(packet: AppExecutionPacket) -> Self {
        Self {
            packet: Arc::new(packet),
            storage: None,
            modules: Vec::new(),
            config: None,
        }
    }

    pub fn with_storage(mut self, storage: StorageKit) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn with_config(mut self, config: HostConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn mount(mut self, module: Arc<dyn HelmModule>) -> Self {
        self.modules.push(module);
        self
    }

    pub async fn build(self) -> Result<BuiltHost> {
        let storage = self
            .storage
            .ok_or_else(|| anyhow::anyhow!("with_storage(...) must be called before build()"))?;
        let config = match self.config {
            Some(c) => c,
            None => HostConfig::from_env(&self.packet),
        };

        let hub = EventHub::new();
        let ctx = HostContext {
            packet: self.packet.clone(),
            storage: storage.clone(),
            realtime: hub.handle(),
        };

        for module in &self.modules {
            module
                .init(&ctx)
                .await
                .map_err(|e| anyhow::anyhow!("module '{}' init failed: {e}", module.module_id()))?;
        }

        let mut router = Router::new()
            .merge(health::router())
            .merge(sse::router(hub.handle()))
            .merge(approvals::router(ApprovalsState {
                packet: self.packet.clone(),
                realtime: hub.handle(),
            }));

        for module in &self.modules {
            router = router.merge(module.clone().router());
        }

        let prefix = self.packet.route_prefix.trim_end_matches('/');
        let router = if prefix.is_empty() {
            router
        } else {
            Router::new().nest(prefix, router)
        };

        Ok(BuiltHost {
            router,
            config,
            modules: self.modules,
            _hub: hub,
        })
    }
}

pub struct BuiltHost {
    router: Router,
    config: HostConfig,
    modules: Vec<Arc<dyn HelmModule>>,
    _hub: EventHub,
}

impl BuiltHost {
    /// Apply an arbitrary transformation to the assembled router before the
    /// server is bound.  Use this to nest additional services (e.g. a static
    /// SPA directory) that cannot be expressed as `HelmModule` implementations.
    ///
    /// The closure receives the fully-assembled router (with route prefix and all
    /// mounted modules already applied) and must return a new `Router`.  Nesting
    /// services here is safe: the domain/API routes registered by modules take
    /// precedence over any fallback / nested service added by the closure.
    pub fn modify_router(mut self, f: impl FnOnce(Router) -> Router) -> Self {
        self.router = f(self.router);
        self
    }

    pub async fn serve(self) -> Result<()> {
        let any_grpc = self
            .modules
            .iter()
            .any(|m| !m.clone().grpc_services().is_empty());
        if any_grpc {
            tracing::warn!(
                "gRPC services declared but server bring-up is not wired in Phase 1 \
                 — modules-only consumers should land before app uses gRPC"
            );
        }

        let addr = format!("0.0.0.0:{}", self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        tracing::info!("runway-app-host listening on http://{addr}");
        axum::serve(listener, self.router).await?;
        Ok(())
    }
}

#[cfg(test)]
impl BuiltHost {
    pub fn into_router_for_test(self) -> Router {
        self.router
    }
}

impl RunwayAppHost {
    pub fn builder(packet: AppExecutionPacket) -> RunwayAppHostBuilder {
        RunwayAppHostBuilder::new(packet)
    }
}
