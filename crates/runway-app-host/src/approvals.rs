use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppExecutionPacket;
use crate::realtime::{EventEnvelope, EventHubHandle};

#[derive(Clone)]
pub struct ApprovalsState {
    pub packet: Arc<AppExecutionPacket>,
    pub realtime: EventHubHandle,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalBody {
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalReceipt {
    pub event_id: Uuid,
}

pub fn router(state: ApprovalsState) -> Router {
    Router::new()
        .route("/v1/approvals/{ref_id}/approve", post(approve))
        .route("/v1/approvals/{ref_id}/reject", post(reject))
        .with_state(state)
}

async fn approve(
    State(state): State<ApprovalsState>,
    Path(ref_id): Path<String>,
    Json(body): Json<ApprovalBody>,
) -> (StatusCode, Json<ApprovalReceipt>) {
    let env = build_envelope(&state, "approval.approved", &ref_id, body);
    state.realtime.publish(env.clone());
    (
        StatusCode::ACCEPTED,
        Json(ApprovalReceipt {
            event_id: env.event_id,
        }),
    )
}

async fn reject(
    State(state): State<ApprovalsState>,
    Path(ref_id): Path<String>,
    Json(body): Json<ApprovalBody>,
) -> (StatusCode, Json<ApprovalReceipt>) {
    let env = build_envelope(&state, "approval.rejected", &ref_id, body);
    state.realtime.publish(env.clone());
    (
        StatusCode::ACCEPTED,
        Json(ApprovalReceipt {
            event_id: env.event_id,
        }),
    )
}

fn build_envelope(
    state: &ApprovalsState,
    ty: &str,
    ref_id: &str,
    body: ApprovalBody,
) -> EventEnvelope {
    EventEnvelope {
        event_id: Uuid::new_v4(),
        sequence: 0,
        r#type: ty.into(),
        schema_version: 1,
        occurred_at: Utc::now(),
        app_id: state.packet.app_id.clone(),
        run_id: None,
        job_id: None,
        correlation_id: body.correlation_id,
        actor: body.actor,
        payload: serde_json::json!({
            "ref": ref_id,
            "note": body.note,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::EventHub;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> ApprovalsState {
        let packet = Arc::new(AppExecutionPacket::new("test", "Test", "test desc", "/"));
        let hub = EventHub::new();
        ApprovalsState {
            packet,
            realtime: hub.handle(),
        }
    }

    #[tokio::test]
    async fn approve_route_returns_202_and_publishes() {
        let state = test_state();
        let mut rx = state.realtime.subscribe();
        let app = router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/approvals/job:42/approve")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"actor\":\"alice\"}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 202);
        let env = rx.recv().await.unwrap();
        assert_eq!(env.r#type, "approval.approved");
        assert_eq!(env.payload["ref"], "job:42");
        assert_eq!(env.actor.as_deref(), Some("alice"));
    }
}
