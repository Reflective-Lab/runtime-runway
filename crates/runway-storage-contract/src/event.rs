//! EventLog (base) and SyncableEventLog contract suites.

use std::sync::Arc;

use chrono::Utc;
use runway_storage::{EventLog, EventQuery, StoredEvent, SyncableEventLog};
use serde_json::json;

use crate::harness::{ContractContext, SuiteReport};
use crate::{contract_assert, contract_test};

fn mk_event(
    ctx: &ContractContext,
    event_type: &str,
    occurred_at: chrono::DateTime<Utc>,
) -> StoredEvent {
    StoredEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        org_id: format!("{}-org", ctx.namespace),
        app_id: format!("{}-app", ctx.namespace),
        event_type: event_type.to_string(),
        context_id: None,
        fact_id: None,
        payload: json!({"hello": "world"}),
        occurred_at,
        synced_at: None,
    }
}

fn base_query(ctx: &ContractContext) -> EventQuery {
    EventQuery {
        org_id: Some(format!("{}-org", ctx.namespace)),
        app_id: Some(format!("{}-app", ctx.namespace)),
        ..Default::default()
    }
}

pub async fn run_event_suite(log: Arc<dyn EventLog>, ctx: ContractContext) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "EventLog");

    contract_test!(&report, "append_then_query_returns_event", async {
        let evt = mk_event(&ctx, "test.append_query", Utc::now());
        let evt_id = evt.event_id.clone();
        log.append(evt).await.map_err(|e| e.to_string())?;
        let events = log
            .query(base_query(&ctx))
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(
            events.iter().any(|e| e.event_id == evt_id),
            "appended event not found in query result"
        );
        Ok(())
    });

    contract_test!(&report, "query_by_event_type_filter", async {
        log.append(mk_event(&ctx, "type.a", Utc::now()))
            .await
            .map_err(|e| e.to_string())?;
        log.append(mk_event(&ctx, "type.b", Utc::now()))
            .await
            .map_err(|e| e.to_string())?;
        let q = EventQuery {
            event_type: Some("type.a".into()),
            ..base_query(&ctx)
        };
        let events = log.query(q).await.map_err(|e| e.to_string())?;
        contract_assert!(
            events.iter().all(|e| e.event_type == "type.a"),
            "type filter leaked other event types"
        );
        Ok(())
    });

    contract_test!(&report, "query_since_filters_by_occurred_at", async {
        let before = Utc::now();
        log.append(mk_event(&ctx, "type.since.before", before))
            .await
            .map_err(|e| e.to_string())?;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let cutoff = Utc::now();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        log.append(mk_event(&ctx, "type.since.after", Utc::now()))
            .await
            .map_err(|e| e.to_string())?;
        let q = EventQuery {
            since: Some(cutoff),
            ..base_query(&ctx)
        };
        let events = log.query(q).await.map_err(|e| e.to_string())?;
        contract_assert!(
            events.iter().all(|e| e.occurred_at >= cutoff),
            "since filter returned earlier events"
        );
        Ok(())
    });

    contract_test!(&report, "query_limit_respected", async {
        for _ in 0..5 {
            log.append(mk_event(&ctx, "type.limit", Utc::now()))
                .await
                .map_err(|e| e.to_string())?;
        }
        let q = EventQuery {
            limit: Some(2),
            event_type: Some("type.limit".into()),
            ..base_query(&ctx)
        };
        let events = log.query(q).await.map_err(|e| e.to_string())?;
        contract_assert!(
            events.len() <= 2,
            "limit not respected: got {} events",
            events.len()
        );
        Ok(())
    });

    contract_test!(&report, "synced_at_none_on_append", async {
        let evt = mk_event(&ctx, "type.synced_at_check", Utc::now());
        let evt_id = evt.event_id.clone();
        log.append(evt).await.map_err(|e| e.to_string())?;
        let events = log
            .query(EventQuery {
                event_type: Some("type.synced_at_check".into()),
                ..base_query(&ctx)
            })
            .await
            .map_err(|e| e.to_string())?;
        let found = events
            .iter()
            .find(|e| e.event_id == evt_id)
            .ok_or("event not found")?;
        contract_assert!(
            found.synced_at.is_none(),
            "freshly-appended event has synced_at set"
        );
        Ok(())
    });

    report
}

pub async fn run_syncable_event_suite(
    log: Arc<dyn SyncableEventLog>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "SyncableEventLog");

    contract_test!(&report, "mark_synced_sets_synced_at", async {
        let evt = mk_event(&ctx, "type.mark_synced", Utc::now());
        let evt_id = evt.event_id.clone();
        log.append(evt).await.map_err(|e| e.to_string())?;
        log.mark_synced(std::slice::from_ref(&evt_id))
            .await
            .map_err(|e| e.to_string())?;
        // The event should no longer appear in unsynced queries.
        let unsynced = log
            .query_unsynced(EventQuery {
                event_type: Some("type.mark_synced".into()),
                ..base_query(&ctx)
            })
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(
            !unsynced.iter().any(|e| e.event_id == evt_id),
            "marked-synced event still in unsynced query"
        );
        // The event record itself must have synced_at populated.
        let all = log
            .query(EventQuery {
                event_type: Some("type.mark_synced".into()),
                ..base_query(&ctx)
            })
            .await
            .map_err(|e| e.to_string())?;
        let found = all
            .iter()
            .find(|e| e.event_id == evt_id)
            .ok_or("marked-synced event not found in query")?;
        contract_assert!(
            found.synced_at.is_some(),
            "mark_synced did not set synced_at on the stored event"
        );
        Ok(())
    });

    contract_test!(&report, "unsynced_only_filters", async {
        log.append(mk_event(&ctx, "type.uns.a", Utc::now()))
            .await
            .map_err(|e| e.to_string())?;
        let synced = mk_event(&ctx, "type.uns.b", Utc::now());
        let synced_id = synced.event_id.clone();
        log.append(synced).await.map_err(|e| e.to_string())?;
        log.mark_synced(std::slice::from_ref(&synced_id))
            .await
            .map_err(|e| e.to_string())?;

        let unsynced = log
            .query_unsynced(EventQuery {
                event_type: Some("type.uns.b".into()),
                ..base_query(&ctx)
            })
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(
            !unsynced.iter().any(|e| e.event_id == synced_id),
            "synced event leaked into unsynced query"
        );
        Ok(())
    });

    report
}
