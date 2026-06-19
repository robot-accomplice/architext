//! Handler for `GET /api/data-events` — Server-Sent Events live-reload stream.
//!
//! Port of the SSE half of `src/adapters/http/data-watch-hub.mjs` (`attach`):
//! every connected client subscribes to the shared [`WatchHub`] broadcast and
//! receives `data: {json}\n\n` frames whenever a settled, validated data change
//! occurs. A keep-alive heartbeat every ~30 s (JS `heartbeatMs = 30000`) keeps
//! the connection open through idle periods and proxies.
//!
//! The loopback-only security middleware already guards this route (it runs on
//! every route), so no extra origin check is needed here.

use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Extension;
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::watch_hub::{DataEvent, WatchHub};

/// JS `heartbeatMs = 30000` — keep-alive comment interval.
const HEARTBEAT_SECS: u64 = 30;

/// GET /api/data-events → an SSE stream of `DataEvent`s.
///
/// If the watch hub is absent (serve started without a data dir to watch — not
/// expected in normal operation), the stream is empty but still well-formed,
/// matching the "connection stays open, no events" degradation.
pub async fn get_data_events(hub: Option<Extension<WatchHub>>) -> impl IntoResponse {
    let stream = data_event_stream(hub.map(|Extension(h)| h));
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(HEARTBEAT_SECS))
            .text("keep-alive"),
    )
}

/// Map the hub broadcast into an SSE event stream. Lagged receivers (a client
/// that fell behind the broadcast buffer) skip the missed frames rather than
/// erroring the whole stream — the next event carries the current `version`, so
/// a skip is self-healing for live-reload. With no hub, the stream is empty but
/// well-formed (the connection still opens and heartbeats keep it alive).
fn data_event_stream(
    hub: Option<WatchHub>,
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    match hub {
        Some(h) => BroadcastStream::new(h.subscribe())
            .filter_map(|item| async move {
                match item {
                    Ok(event) => Some(Ok(sse_event(&event))),
                    // BroadcastStreamRecvError::Lagged → drop and continue.
                    Err(_) => None,
                }
            })
            .boxed(),
        None => futures::stream::empty().boxed(),
    }
}

/// Serialize a `DataEvent` into an SSE `data:` frame. Serialization of the small
/// fixed-shape payload cannot fail in practice; if it ever did we fall back to a
/// minimal invalid-shaped frame rather than dropping the connection.
fn sse_event(event: &DataEvent) -> Event {
    match serde_json::to_string(event) {
        Ok(json) => Event::default().data(json),
        Err(_) => Event::default().data(
            r#"{"type":"invalid","version":0,"output":"event serialization failed"}"#,
        ),
    }
}
