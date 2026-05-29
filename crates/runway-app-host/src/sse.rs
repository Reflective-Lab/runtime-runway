use std::convert::Infallible;

use axum::{
    Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::realtime::EventHubHandle;

pub fn router(hub: EventHubHandle) -> Router {
    Router::new()
        .route("/sse/stream", get(stream))
        .with_state(hub)
}

async fn stream(
    State(hub): State<EventHubHandle>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = hub.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|ev| async move {
        ev.ok().and_then(|env| {
            serde_json::to_string(&env)
                .ok()
                .map(|s| Ok(Event::default().event(env.r#type.clone()).data(s)))
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::EventHub;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn sse_endpoint_is_reachable() {
        let hub = EventHub::new();
        let app = router(hub.handle());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/sse/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().starts_with("text/event-stream"));
    }
}
