//! Data-watch hub — the Rust port of `src/adapters/http/data-watch-hub.mjs`.
//!
//! Watches `{data_dir}/**/*.json` for changes (the `notify` crate, recursive),
//! DEBOUNCES bursts until they settle (~300 ms, JS `settleMs`), then validates
//! the data dir via [`architext_core::validate_data_dir`] and BROADCASTS a
//! `DataEvent` to every connected SSE client over a [`tokio::sync::broadcast`]
//! channel.
//!
//! Event payload shape is faithful to the JS hub
//! (`data-watch-hub.mjs` `validateAndBroadcast`):
//!
//! ```json
//! { "type": "valid" | "invalid", "version": <n>, "output": "<validator text>" }
//! ```
//!
//! The SSE response wiring (heartbeat, keep-alive, per-client subscribe) lives
//! in `handlers/data_events.rs`; this module is the watch + validate + broadcast
//! engine only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;

use architext_core::{validate_data_dir, ValidationOutcome};

/// JS `settleMs = 300` — debounce window before a settled change is validated.
pub const SETTLE_MS: u64 = 300;

/// A single broadcast event, serialized to the SSE `data:` line.
///
/// Matches the JS payload exactly: `{ type, version, output }`. `type` is
/// `"valid"` when validation passed, `"invalid"` otherwise.
#[derive(Debug, Clone, Serialize)]
pub struct DataEvent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub version: u64,
    pub output: String,
}

/// Render the validator's human-readable `output` text, faithful to the JS
/// validator CLI (`viewer/tools/validate-architext.mjs`):
///   - pass → `"Architext validation passed."`
///   - fail → `"Architext validation failed:\n- <error>\n- <error>…"`
fn validation_output_text(outcome: &ValidationOutcome) -> String {
    if outcome.ok {
        "Architext validation passed.".to_string()
    } else {
        let mut text = String::from("Architext validation failed:");
        for error in &outcome.errors {
            text.push_str("\n- ");
            text.push_str(error);
        }
        text
    }
}

/// Build the broadcast event for a validation outcome at a given version.
/// Pure (no I/O) so the classify+shape contract is unit-testable.
pub fn build_event(outcome: &ValidationOutcome, version: u64) -> DataEvent {
    DataEvent {
        kind: if outcome.ok { "valid" } else { "invalid" },
        version,
        output: validation_output_text(outcome),
    }
}

/// The live watch hub. Cloneable handle: `subscribe()` hands each SSE client a
/// `broadcast::Receiver`. Keeps the `notify` watcher alive for the hub's
/// lifetime (dropping the hub stops the watcher — best-effort clean shutdown).
#[derive(Clone)]
pub struct WatchHub {
    tx: tokio::sync::broadcast::Sender<DataEvent>,
    /// Kept alive so the OS watch handle is not dropped; never read directly.
    _watcher: Arc<RecommendedWatcher>,
}

impl WatchHub {
    /// Subscribe a new SSE client to the broadcast stream.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DataEvent> {
        self.tx.subscribe()
    }
}

/// Run a settled, validated broadcast for `data_dir`/`schema_dir`, bumping the
/// shared version counter first (JS `version += 1` before broadcast). Returns
/// the event that was sent (also useful for tests).
fn validate_and_broadcast(
    data_dir: &Path,
    schema_dir: &Path,
    version: &AtomicU64,
    tx: &tokio::sync::broadcast::Sender<DataEvent>,
) -> DataEvent {
    let outcome = validate_data_dir(data_dir, schema_dir);
    let v = version.fetch_add(1, Ordering::SeqCst) + 1;
    let event = build_event(&outcome, v);
    // A send error only means no receivers are currently attached; that is not
    // a failure of the watch loop (JS broadcasts to whatever clients exist).
    let _ = tx.send(event.clone());
    event
}

/// Start the watch hub: install a recursive `notify` watcher over `data_dir`,
/// and spawn a debounce/validate/broadcast task. Returns a cloneable [`WatchHub`]
/// whose `subscribe()` feeds SSE clients.
///
/// Debounce semantics mirror the JS hub: a `.json` change (re)arms a `SETTLE_MS`
/// timer; only when the burst settles do we validate once and broadcast.
/// Non-`.json` paths are ignored (JS `if (fileName && !fileName.endsWith(".json")) return`).
pub fn start_watch_hub(data_dir: PathBuf, schema_dir: PathBuf) -> notify::Result<WatchHub> {
    let (tx, _rx) = tokio::sync::broadcast::channel::<DataEvent>(64);
    let version = Arc::new(AtomicU64::new(0));

    // notify → tokio bridge: the watcher callback runs on notify's own thread,
    // so it can only signal the async debounce task via an mpsc channel.
    let (change_tx, mut change_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // Only react to actual content/metadata mutations of .json files.
        let touches_json = event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|ext| ext == "json"));
        if touches_json {
            // Ignore send errors: the debounce task has gone away → nothing to do.
            let _ = change_tx.send(());
        }
    })?;
    watcher.watch(&data_dir, RecursiveMode::Recursive)?;

    // Debounce + validate + broadcast loop. Holds clones of the paths, the
    // version counter, and the broadcast sender.
    let loop_tx = tx.clone();
    tokio::spawn(async move {
        while change_rx.recv().await.is_some() {
            // Coalesce the burst: keep resetting the settle window while changes
            // keep arriving, then validate once. `recv` with a timeout is the
            // debounce primitive — another change inside the window (`Ok(Some)`)
            // re-arms it; the window elapsing (`Err`) or the channel closing
            // (`Ok(None)`) ends the wait and lets us validate.
            while let Ok(Some(())) =
                tokio::time::timeout(Duration::from_millis(SETTLE_MS), change_rx.recv()).await
            {}
            // Validation touches the filesystem; keep it off the async reactor.
            let dd = data_dir.clone();
            let sd = schema_dir.clone();
            let ver = version.clone();
            let send = loop_tx.clone();
            let _ = tokio::task::spawn_blocking(move || {
                validate_and_broadcast(&dd, &sd, &ver, &send);
            })
            .await;
        }
    });

    Ok(WatchHub {
        tx,
        _watcher: Arc::new(watcher),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn schema_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap() // crates/
            .parent()
            .unwrap() // repo root
            .join("viewer")
            .join("schema")
    }

    /// A minimal valid data dir copied from the real self-hosted docs data.
    fn real_data_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("docs")
            .join("architext")
            .join("data")
    }

    #[test]
    fn build_event_shape_for_valid_outcome() {
        let outcome = ValidationOutcome { ok: true, errors: vec![] };
        let event = build_event(&outcome, 7);
        assert_eq!(event.kind, "valid");
        assert_eq!(event.version, 7);
        assert_eq!(event.output, "Architext validation passed.");
        // Faithful JSON shape: { "type", "version", "output" }.
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "valid");
        assert_eq!(json["version"], 7);
        assert_eq!(json["output"], "Architext validation passed.");
    }

    #[test]
    fn build_event_shape_for_invalid_outcome() {
        let outcome = ValidationOutcome {
            ok: false,
            errors: vec!["nodes contains duplicate id \"a\"".into(), "bad ref".into()],
        };
        let event = build_event(&outcome, 2);
        assert_eq!(event.kind, "invalid");
        assert_eq!(event.version, 2);
        assert_eq!(
            event.output,
            "Architext validation failed:\n- nodes contains duplicate id \"a\"\n- bad ref"
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "invalid");
    }

    /// End-to-end hub test: a JSON change in a temp data dir produces a settled
    /// `valid` event with the right shape over the broadcast channel.
    #[tokio::test]
    async fn hub_emits_valid_event_on_json_change() {
        // Copy the real data dir into a temp dir so an edit there doesn't dirty
        // the repo (and so the dataset is genuinely valid).
        let src = real_data_dir();
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("data");
        copy_dir_recursive(&src, &dst);

        let hub = start_watch_hub(dst.clone(), schema_dir()).expect("watch hub starts");
        let mut rx = hub.subscribe();

        // Touch a JSON file (harmless whitespace append then truncate-rewrite is
        // overkill; a metadata-only re-write of the same bytes still fires notify).
        let manifest = dst.join("manifest.json");
        let bytes = fs::read(&manifest).unwrap();
        // Small delay so the watcher is fully registered before the write.
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::write(&manifest, &bytes).unwrap();

        // Wait for the settled broadcast (debounce 300ms + validate). Generous
        // timeout to stay robust on slow CI filesystems.
        let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("event arrives before timeout")
            .expect("broadcast channel open");

        assert_eq!(event.kind, "valid", "real data dir validates clean");
        assert!(event.version >= 1);
        assert_eq!(event.output, "Architext validation passed.");
    }

    /// An invalid edit produces an `invalid` event whose output carries the
    /// validator failure text (last-good is preserved by the consumer, not here).
    #[tokio::test]
    async fn hub_emits_invalid_event_on_bad_json_change() {
        let src = real_data_dir();
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("data");
        copy_dir_recursive(&src, &dst);

        let hub = start_watch_hub(dst.clone(), schema_dir()).expect("watch hub starts");
        let mut rx = hub.subscribe();

        // Corrupt nodes.json into invalid JSON.
        let nodes = dst.join("nodes.json");
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::write(&nodes, b"{ this is not valid json").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("event arrives before timeout")
            .expect("broadcast channel open");

        assert_eq!(event.kind, "invalid");
        assert!(
            event.output.starts_with("Architext validation failed:"),
            "got: {}",
            event.output
        );
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let target = dst.join(entry.file_name());
            if path.is_dir() {
                copy_dir_recursive(&path, &target);
            } else {
                fs::copy(&path, &target).unwrap();
            }
        }
    }
}
