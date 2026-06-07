use std::sync::Arc;

use async_trait::async_trait;
use axum::{Router, routing::get};
use runway_app_host::{
    AppExecutionPacket, BuiltHost, HelmModule, HostConfig, HostContext, RunwayAppHost,
};
use runway_storage::StorageKit;
use tempfile::TempDir;

struct NoopModule {
    received_packet_id: tokio::sync::OnceCell<String>,
}

#[async_trait]
impl HelmModule for NoopModule {
    fn module_id(&self) -> &'static str {
        "test.noop"
    }

    async fn init(&self, ctx: &HostContext) -> anyhow::Result<()> {
        self.received_packet_id
            .set(ctx.packet.app_id.clone())
            .map_err(|_| anyhow::anyhow!("init called twice"))?;
        Ok(())
    }

    fn router(self: Arc<Self>) -> Router {
        Router::new().route("/v1/noop/ping", get(|| async { "pong" }))
    }
}

/// Bind port 0 on loopback to let the OS pick a free port, then immediately
/// release the socket and return the port number. There is a small TOCTOU
/// window, but it is acceptable for tests on a developer machine.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn build_host(packet: AppExecutionPacket, port: u16) -> BuiltHost {
    let tmp = TempDir::new().unwrap();
    let storage = StorageKit::local(tmp.path()).await.unwrap();
    RunwayAppHost::builder(packet)
        .with_storage(storage)
        .with_config(HostConfig {
            port,
            ..HostConfig::default()
        })
        .build()
        .await
        .unwrap()
}

#[tokio::test]
async fn build_invokes_module_init_in_order() {
    let tmp = TempDir::new().unwrap();
    let storage = StorageKit::local(tmp.path()).await.unwrap();

    let packet = AppExecutionPacket::new("test-app", "Test", "desc", "/");
    let module = Arc::new(NoopModule {
        received_packet_id: tokio::sync::OnceCell::new(),
    });

    let host = RunwayAppHost::builder(packet)
        .with_storage(storage)
        .with_config(HostConfig::default())
        .mount(module.clone())
        .build()
        .await
        .unwrap();

    assert_eq!(
        module.received_packet_id.get().map(String::as_str),
        Some("test-app")
    );
    drop(host);
}

#[tokio::test]
async fn host_mounts_canonical_routes() {
    let port = free_port();
    let packet = AppExecutionPacket::new("test-app", "Test", "desc", "/");
    let host = build_host(packet, port).await;

    // Spawn the server in the background; abort when the test ends.
    let task = tokio::spawn(async move { host.serve().await });

    // Give the listener a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // RP-HERMETIC-UNIT (Reflective QUALITY_BACKLOG.md →
    // QF-2026-06-02-05): this integration test spins up a local Axum
    // server on 127.0.0.1 and hits it via a real reqwest client. No
    // external network — the client talks only to the in-process
    // listener bound a few lines above. Hermetic by construction.
    #[allow(clippy::disallowed_methods)]
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    for path in ["/healthz", "/sse/stream"] {
        let resp = client
            .get(format!("{base}{path}"))
            .send()
            .await
            .unwrap_or_else(|e| panic!("{path} request failed: {e}"));
        assert!(
            resp.status().is_success(),
            "{path} should be 2xx, got {}",
            resp.status()
        );
    }

    task.abort();
}
