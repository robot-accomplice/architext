//! Live-reload: consume the serve `/api/data-events` SSE stream.
//!
//! Counterpart to the serve-side data-watch hub
//! (`crates/architext-serve/src/watch_hub.rs`). The hub broadcasts a settled,
//! validated `{ type, version, output }` payload on every on-disk data change;
//! this module opens an [`web_sys::EventSource`] to that endpoint and reacts:
//!
//!   - `type: "valid"`   → re-run the V2 load ([`super::load_architecture_data`])
//!     and swap the new dataset into the `AppState` data signal, PRESERVING the
//!     user's current mode/view/flow selection (see `AppState::reload_data`).
//!   - `type: "invalid"` → surface a non-blocking notice carrying the validator
//!     output; the last-good diagram keeps rendering.
//!
//! `EventSource` reconnects automatically on a dropped connection (its built-in
//! retry), so we don't spin up our own reconnect loop — `onerror`/`onopen` only
//! toggle the "live" indicator. The single `EventSource` is `forget()`-leaked
//! intentionally: it must outlive this function for the lifetime of the app.

use serde::Deserialize;

#[cfg(target_arch = "wasm32")]
use leptos::{spawn_local, SignalSet};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use web_sys::{EventSource, MessageEvent};

#[cfg(target_arch = "wasm32")]
use crate::data::load_architecture_data;
use crate::state::AppState;

/// The SSE endpoint served by the data-watch hub. Same-origin relative URL so it
/// works wherever the server mounts the viewer.
#[cfg(target_arch = "wasm32")]
const DATA_EVENTS_URL: &str = "/api/data-events";

/// The broadcast payload, faithful to the serve hub's `DataEvent`
/// (`{ "type": "valid" | "invalid", "version": <n>, "output": "<text>" }`).
///
/// `allow(dead_code)`: the fields are consumed by the wasm32 handlers and the
/// unit tests; a non-test native build sees them as unused.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DataEvent {
    #[serde(rename = "type")]
    kind: String,
    version: u64,
    output: String,
}

/// Native no-op: the EventSource consumer only exists on wasm32. Keeps `App`'s
/// call site target-agnostic so `cargo test --lib` (native) compiles.
#[cfg(not(target_arch = "wasm32"))]
pub fn start_live_reload(_state: AppState) {}

/// Open the live-reload SSE connection and wire its handlers into `state`.
///
/// Best-effort: if the browser can't construct an `EventSource` (or the endpoint
/// is unavailable), the viewer simply runs without live-reload — never an error
/// surface. Idempotent enough for a single call at app start.
#[cfg(target_arch = "wasm32")]
pub fn start_live_reload(state: AppState) {
    let source = match EventSource::new(DATA_EVENTS_URL) {
        Ok(s) => s,
        // No live-reload available; the static dataset keeps working.
        Err(_) => return,
    };

    wire_open(&source, state);
    wire_error(&source, state);
    wire_message(&source, state);

    // The EventSource must outlive this function. Leak the handle deliberately;
    // it lives for the app's lifetime (the page owns exactly one).
    std::mem::forget(source);
}

/// `onopen` → mark the live indicator connected.
#[cfg(target_arch = "wasm32")]
fn wire_open(source: &EventSource, state: AppState) {
    let on_open = Closure::<dyn FnMut()>::new(move || {
        state.live_connected.set(true);
    });
    source.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();
}

/// `onerror` → mark the live indicator disconnected. EventSource will retry on
/// its own; we don't reconnect manually (that would double-connect).
#[cfg(target_arch = "wasm32")]
fn wire_error(source: &EventSource, state: AppState) {
    let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_evt: web_sys::Event| {
        state.live_connected.set(false);
    });
    source.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();
}

/// `onmessage` → parse the payload and dispatch valid/invalid handling.
#[cfg(target_arch = "wasm32")]
fn wire_message(source: &EventSource, state: AppState) {
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(text) = evt.data().as_string() else { return };
        let Ok(event) = serde_json::from_str::<DataEvent>(&text) else { return };
        handle_event(state, event);
    });
    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
}

/// Dispatch a parsed event: re-fetch on valid, surface a notice on invalid.
#[cfg(target_arch = "wasm32")]
fn handle_event(state: AppState, event: DataEvent) {
    match event.kind.as_str() {
        "valid" => {
            // Re-run the V2 load, then swap the dataset in (preserving selection).
            spawn_local(async move {
                match load_architecture_data().await {
                    Ok(data) => state.reload_data(data),
                    // A transient fetch failure right after a valid event is rare
                    // (the data just validated on the server). Surface it as a
                    // notice rather than blanking the last-good diagram.
                    Err(err) => state
                        .invalid_notice
                        .set(Some(format!("Live reload failed to fetch data: {err}"))),
                }
            });
        }
        // "invalid" (or any non-valid type) → non-blocking notice; keep last-good.
        _ => {
            state.invalid_notice.set(Some(invalid_summary(&event.output)));
        }
    }
}

/// Condense the validator output into a single-line notice summary. The full
/// multi-line output stays in the payload; the banner shows the first error line
/// (or the headline) so it fits the hairline notice strip.
///
/// `allow(dead_code)`: used by the wasm32 `handle_event` and the unit tests; a
/// non-test native build sees it as unused.
#[allow(dead_code)]
fn invalid_summary(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("- "))
        .map(|line| line.trim_start_matches("- ").to_string())
        .or_else(|| output.lines().next().map(str::to_string))
        .unwrap_or_else(|| "Data invalid".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_summary_picks_first_error_line() {
        let output = "Architext validation failed:\n- nodes contains duplicate id \"a\"\n- bad ref";
        assert_eq!(invalid_summary(output), "nodes contains duplicate id \"a\"");
    }

    #[test]
    fn invalid_summary_falls_back_to_headline() {
        assert_eq!(invalid_summary("Something went wrong"), "Something went wrong");
    }

    #[test]
    fn data_event_deserializes_faithful_shape() {
        let json = r#"{"type":"valid","version":3,"output":"Architext validation passed."}"#;
        let event: DataEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.kind, "valid");
        assert_eq!(event.version, 3);
        assert_eq!(event.output, "Architext validation passed.");
    }
}
