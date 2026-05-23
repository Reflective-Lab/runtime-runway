use axum::{
    Json, Router,
    extract::Request,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::Span;
use uuid::Uuid;

/// Runtime configuration for the middleware stack.
///
/// Populated by the binary at startup and passed into `stack`. The
/// middleware crate itself never reads from the process environment.
#[derive(Debug, Clone, Default)]
pub struct MiddlewareConfig {
    /// Comma-separated list of allowed CORS origins. Empty = allow any
    /// (the local-development default; production binaries must populate
    /// this from `ALLOWED_ORIGINS`).
    pub allowed_origins: String,
}

/// Attach the full middleware stack to any Axum router.
///
/// Order matters: request-id is outermost, compression is innermost.
pub fn stack<S>(router: Router<S>, cfg: &MiddlewareConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route("/health", get(health)).layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|req: &Request<_>| {
                        let request_id = req
                            .headers()
                            .get("x-request-id")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("-");
                        tracing::info_span!(
                            "request",
                            method = %req.method(),
                            uri    = %req.uri(),
                            request_id,
                        )
                    })
                    .on_response(
                        |resp: &Response<_>, latency: std::time::Duration, _span: &Span| {
                            tracing::info!(
                                status = resp.status().as_u16(),
                                latency_ms = latency.as_millis(),
                                "response"
                            );
                        },
                    ),
            )
            .layer(CompressionLayer::new())
            .layer(
                CorsLayer::new()
                    .allow_methods(Any)
                    .allow_headers(Any)
                    .allow_origin(cors_origin(&cfg.allowed_origins)),
            )
            .layer(middleware::from_fn(error_formatter)),
    )
}

/// Serve the router on the given port with graceful SIGTERM shutdown.
///
/// Call `.with_state()` on your router before passing it here. The
/// binary is responsible for resolving the port (Cloud Run sets `PORT`
/// in the env; local dev defaults to 8080) — this crate no longer reads
/// the environment.
pub async fn serve(app: Router, port: u16) {
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind failed");
    tracing::info!("listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Catch unhandled errors and return a clean JSON body (no stack trace to client).
async fn error_formatter(req: Request, next: Next) -> Response {
    let resp = next.run(req).await;
    if resp.status().is_server_error() {
        let status = resp.status();
        return (
            status,
            Json(json!({
                "error": status.canonical_reason().unwrap_or("internal error"),
                "request_id": Uuid::new_v4().to_string(),
            })),
        )
            .into_response();
    }
    resp
}

fn cors_origin(allowed_origins: &str) -> tower_http::cors::AllowOrigin {
    if allowed_origins.is_empty() {
        return tower_http::cors::AllowOrigin::any();
    }
    let parsed: Vec<_> = allowed_origins
        .split(',')
        .filter_map(|o| o.trim().parse::<axum::http::HeaderValue>().ok())
        .collect();
    tower_http::cors::AllowOrigin::list(parsed)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("ctrl-c handler failed");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler failed")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c   => tracing::info!("ctrl-c received, shutting down"),
        _ = terminate => tracing::info!("SIGTERM received, shutting down"),
    }
}
