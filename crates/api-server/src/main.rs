use std::sync::Arc;

use anyhow::Result;
use axum::{Extension, Json, Router, extract::State, http::StatusCode, routing::get};
use chrono::Utc;
use runway_accounts::AccountsState;
use runway_auth::{AuthContext, AuthLayer, FirebaseAuth};
use runway_middleware::{serve, stack};
use runway_storage::{
    StorageKit, StoredEvent,
    remote::{RemoteConfig, RemoteStorageKit},
    traits::event::EventQuery,
};
use runway_telemetry::{TelemetryConfig, init as init_telemetry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    storage: Arc<StorageKit>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _telemetry = init_telemetry(TelemetryConfig::from_env("api-server"))?;

    let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");

    let storage = if local_dev {
        let base = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "/tmp/api-server".into());
        StorageKit::local(base).await?
    } else {
        RemoteStorageKit::build(RemoteConfig::from_env()?).await?
    };

    let storage = Arc::new(storage);

    if !local_dev {
        assert!(
            std::env::var("STRIPE_WEBHOOK_SECRET")
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            "STRIPE_WEBHOOK_SECRET must be set in production (empty value disables HMAC verification)"
        );
        assert!(
            std::env::var("ALLOWED_ORIGINS")
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            "ALLOWED_ORIGINS must be set in production (e.g. https://apps.reflective.se)"
        );
    }

    let project_id = std::env::var("FIREBASE_PROJECT_ID").unwrap_or_else(|_| "dev-project".into());
    let auth = FirebaseAuth::new(project_id);
    let auth_layer = AuthLayer::new(auth, local_dev);

    let accounts = AccountsState::new(Arc::clone(&storage));

    // Public routes: no auth required.
    let public = Router::new()
        .route("/status", get(status))
        .merge(runway_accounts::public_routes(accounts.clone()));

    // Protected API routes — served with AppState.
    let api_protected: Router<()> = Router::new()
        .route("/api/me", get(me))
        .route("/api/events", get(list_events).post(append_event))
        .with_state(AppState {
            storage: Arc::clone(&storage),
        });

    // Protected accounts routes — served with AccountsState (already called with_state).
    let accounts_protected: Router<()> = runway_accounts::protected_routes(accounts);

    // Merge all protected routes then apply the auth layer once.
    let protected = api_protected.merge(accounts_protected).layer(auth_layer);

    // ROUTE_PREFIX=/api-server mounts all routes under that path.
    // Firebase Hosting rewrites pass the full path through, so this lets
    // apps.reflective.se/api-server/** route to this service.
    // /health always stays at root for Cloud Run health checks.
    let routed = match std::env::var("ROUTE_PREFIX") {
        Ok(prefix) if !prefix.is_empty() => Router::new().nest(&prefix, public.merge(protected)),
        _ => public.merge(protected),
    };

    let app = stack(routed);

    info!("api-server starting");
    serve(app).await;
    Ok(())
}

async fn status() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "api-server",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn me(Extension(ctx): Extension<AuthContext>) -> Json<Value> {
    Json(json!({
        "uid": ctx.uid(),
        "org_id": ctx.org_id(),
    }))
}

#[derive(Deserialize)]
struct EventQueryParams {
    org_id: Option<String>,
    app_id: Option<String>,
    event_type: Option<String>,
    limit: Option<usize>,
}

async fn list_events(
    Extension(ctx): Extension<AuthContext>,
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<EventQueryParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let org_id = params.org_id.or_else(|| ctx.org_id().map(str::to_owned));

    let query = EventQuery {
        org_id,
        app_id: params.app_id,
        event_type: params.event_type,
        limit: params.limit,
        unsynced_only: false,
        ..Default::default()
    };

    state
        .storage
        .events
        .query(query)
        .await
        .map(|events| Json(json!({ "events": events })))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })
}

#[derive(Deserialize, Serialize)]
struct AppendRequest {
    org_id: Option<String>,
    app_id: String,
    event_type: String,
    payload: Value,
}

async fn append_event(
    Extension(ctx): Extension<AuthContext>,
    State(state): State<AppState>,
    Json(req): Json<AppendRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let org_id = req
        .org_id
        .or_else(|| ctx.org_id().map(str::to_owned))
        .unwrap_or_default();

    let event = StoredEvent {
        event_id: Uuid::new_v4().to_string(),
        org_id,
        app_id: req.app_id,
        event_type: req.event_type,
        context_id: None,
        fact_id: None,
        payload: req.payload,
        occurred_at: Utc::now(),
        synced_at: None,
    };

    let id = event.event_id.clone();
    state
        .storage
        .events
        .append(event)
        .await
        .map(|()| Json(json!({ "id": id })))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })
}
