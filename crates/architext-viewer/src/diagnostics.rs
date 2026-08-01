//! RCA instrumentation for the code-graph view's teardown/rebuild lifecycle
//! (Rule 14: "a new failure surface without recorded-state coverage is
//! incomplete work, like code without tests").
//!
//! WHY THIS EXISTS: `components/code_graph_view.rs`'s module doc describes
//! the `alive: Rc<RefCell<bool>>` guard that every callback checks before
//! touching a Leptos signal, because `canvas_panel.rs`'s outer mode-render
//! closure can tear the component down and rebuild it more than once per
//! `set_mode()`. Every one of those guards used to return SILENTLY. A real
//! browser session hit a state where `alive == false` while the component's
//! DOM was still mounted and receiving clicks — the inspector stopped
//! updating and the RAF loop had stalled — and there was no recorded state
//! to tell "torn-down instance N, nothing replaced it" apart from "instance
//! N died and instance N+1 mounted but something else broke". This module
//! closes that gap: every mount, cleanup, alive-guarded bail, layout-settle
//! transition, selection-mirror write, and RAF stall is recorded BOTH to the
//! live console (`console.info`, for a debugger attached right now) AND to a
//! bounded `localStorage` ring buffer (for reading minutes later, which is
//! how the original failure was actually noticed).
//!
//! RETRIEVAL (zero JavaScript — no devtools breakpoint needed): open the
//! browser console on the running viewer and call
//!
//! ```text
//! window.wasmBindings.architext_diagnostics()
//! ```
//!
//! `window.wasmBindings` is Trunk's own generated glue (`index.html`'s
//! `<script type="module">` does `window.wasmBindings = bindings;` after
//! `init()` — see `dist/index.html`), not anything hand-written here; every
//! `#[wasm_bindgen]`-exported function in this crate lands on it.
//! `architext_diagnostics()` (defined in `lib.rs`, thin wrapper over
//! [`dump`]) returns the buffer as a pretty-printed JSON array string,
//! oldest entry first.
//!
//! BOUNDING: `localStorage` is ~5MB and shared with the theme key
//! (`theme.rs`) — instrumentation must never be the thing that breaks the
//! viewer. The ring buffer caps at [`MAX_ENTRIES`] entries (oldest evicted
//! first) and every storage call is failure-tolerant (private browsing,
//! quota exceeded, disabled storage all degrade to "diagnostics didn't
//! persist this time", never a panic or a broken app).
//!
//! ALIVE-BAIL THROTTLING: once `alive` flips false it never flips back — a
//! torn-down component instance stays dead for the rest of the page's life.
//! So every subsequent call into any alive-guarded callback for that
//! instance bails, forever. Recording every one of those (especially the
//! per-frame RAF loop and the per-settle-tick `full_upload`/`sync_and_upload`
//! paths) would blow through the ring buffer in a couple of seconds and bury
//! the one entry that matters under thousands of identical repeats that add
//! no new information — knowing call site X bailed ONCE after teardown
//! already tells you it was reached; the 400th identical bail tells you
//! nothing more. [`record_alive_bail`] therefore records only the FIRST bail
//! per (instance, call site) pair; see its doc for the mechanism.
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// `localStorage` key for the ring buffer (namespaced like `theme.rs`'s
/// `THEME_STORAGE_KEY` so the two features never collide).
const STORAGE_KEY: &str = "architext-diagnostics";

/// Ring buffer capacity — "a few hundred entries", per the instrumentation
/// budget: enough to cover a mount/cleanup/settle cycle or two even with a
/// stalled RAF loop, small enough that the worst case (every entry near the
/// largest realistic `detail` string) stays well under a tiny fraction of
/// `localStorage`'s ~5MB budget.
const MAX_ENTRIES: usize = 300;

/// Sentinel instance id for events that are not scoped to a live
/// `CodeGraphViewCanvas` instance — currently only the app-load background
/// layout warm (`layout_worker_client::warm_function_tier`), which starts
/// before any Code Graph view exists. Real instances start at 1 (see
/// [`next_instance_id`]), so 0 is never ambiguous with a real one.
pub const NO_INSTANCE: u32 = 0;

/// Process-wide monotonic instance counter (item 1 of the instrumentation
/// list): every event carries the id of the component instance that
/// produced it, so the diagnostics dump can tell "instance 1 was torn down
/// and instance 2 mounted cleanly" apart from "instance 1 died and nothing
/// replaced it" — the exact ambiguity the original failure could not
/// resolve.
static NEXT_INSTANCE_ID: AtomicU32 = AtomicU32::new(1);

/// Mint the next component-instance id. Called once per `CodeGraphViewCanvas`
/// mount, before anything else in the component body runs.
pub fn next_instance_id() -> u32 {
    NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Monotonic event sequence number — orders events unambiguously even if two
/// share the same millisecond timestamp (`Date.now()` resolution is coarser
/// than a single animation frame).
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// One recorded lifecycle event. Deliberately flat and small: a `seq`/`ts_ms`
/// pair for ordering, the instance it belongs to, a short static event name,
/// and one free-form `detail` line (already-formatted `key=value` facts, not
/// a nested structure) — matching the "keep each entry small" budget rather
/// than inventing a per-event schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiagEvent {
    pub seq: u64,
    pub ts_ms: f64,
    pub instance: u32,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// Storage backend seam. Exists so the eviction/persistence logic below can
/// be unit-tested on the native `cargo test` host target: calling ANY real
/// `web_sys` function there panics ("cannot access imported statics on
/// non-wasm targets", confirmed directly), so tests substitute
/// [`tests::FakeStorage`] instead of touching `localStorage` at all.
trait DiagStorage {
    fn read(&self) -> Option<String>;
    /// `Err` means the write did not happen (quota exceeded, private
    /// browsing, storage disabled) — callers must swallow it, never let a
    /// storage failure propagate out of the diagnostics module.
    fn write(&self, value: &str) -> Result<(), ()>;
}

/// Real `localStorage`, same access pattern as `theme.rs`'s `local_storage()`
/// (module-private helper here, deliberately not shared — `theme.rs` has no
/// reason to depend on this module or vice versa).
struct BrowserStorage;

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl DiagStorage for BrowserStorage {
    fn read(&self) -> Option<String> {
        local_storage()?.get_item(STORAGE_KEY).ok().flatten()
    }

    fn write(&self, value: &str) -> Result<(), ()> {
        let storage = local_storage().ok_or(())?;
        storage.set_item(STORAGE_KEY, value).map_err(|_| ())
    }
}

/// Push one entry and evict from the front until the buffer is back at or
/// under `cap`. Pure logic — the eviction policy itself, unit-tested below
/// without any storage backend involved.
fn push_bounded(buf: &mut VecDeque<DiagEvent>, cap: usize, entry: DiagEvent) {
    buf.push_back(entry);
    while buf.len() > cap {
        buf.pop_front();
    }
}

/// Reconstruct the buffer from whatever storage returns — a missing key,
/// unreadable storage, or corrupt JSON (e.g. a stale shape from a previous
/// version of this module) all collapse to "start empty" rather than a
/// panic or a propagated error.
fn load_buffer<S: DiagStorage>(storage: &S) -> VecDeque<DiagEvent> {
    storage
        .read()
        .and_then(|raw| serde_json::from_str::<VecDeque<DiagEvent>>(&raw).ok())
        .unwrap_or_default()
}

/// Serialize and write the buffer, tolerating any failure (see
/// `DiagStorage::write`'s doc) — a failed write just means this call's
/// entries won't be in the NEXT read of the persisted trail; the live
/// `console.info` line (`record`, above `persist` in the call chain) has
/// already gone out regardless, so nothing is silently lost from the
/// debugger's point of view, only from the after-the-fact one. Kept free of
/// any `web_sys` call itself (not even a `console.warn` on failure) so this
/// function stays testable against [`tests::FakeStorage`] on the native
/// `cargo test` host target, where any real `web_sys` call panics.
fn persist<S: DiagStorage>(storage: &S, buf: &VecDeque<DiagEvent>) {
    let Ok(json) = serde_json::to_string(buf) else { return };
    let _ = storage.write(&json);
}

thread_local! {
    /// Lazily loaded from storage on first use per wasm instance, then kept
    /// in memory (avoids a `localStorage` round-trip on every single event).
    static BUFFER: RefCell<Option<VecDeque<DiagEvent>>> = const { RefCell::new(None) };
    /// (instance, call site) pairs already recorded as an alive-bail — see
    /// the module doc's "ALIVE-BAIL THROTTLING" section.
    static ALIVE_BAILS_SEEN: RefCell<HashSet<(u32, &'static str)>> = RefCell::new(HashSet::new());
}

/// Wall-clock milliseconds. `js_sys::Date::now()`, NOT `std::time` — the
/// latter panics on wasm32 (no OS clock syscall), and `Performance::now()`
/// is deliberately avoided here too: it is monotonic-since-navigation-start,
/// not a wall-clock timestamp, and these entries need to correlate against
/// the ordinary browser-console `console.info` timestamps a person reads
/// them alongside minutes later.
fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// Record one lifecycle event: `console.info` immediately (live debugging)
/// and the bounded `localStorage` ring buffer (after-the-fact reading — see
/// module doc). Every call site below passes a short, already-formatted
/// `detail` string rather than building a nested payload, keeping entries
/// small by construction.
pub fn record(instance: u32, event: &str, detail: Option<String>) {
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    let ts_ms = now_ms();

    let line = match &detail {
        Some(d) => format!("[diag] seq={seq} t={ts_ms:.0} inst={instance} {event} {d}"),
        None => format!("[diag] seq={seq} t={ts_ms:.0} inst={instance} {event}"),
    };
    web_sys::console::info_1(&line.into());

    let entry = DiagEvent { seq, ts_ms, instance, event: event.to_string(), detail };
    BUFFER.with(|cell| {
        let mut guard = cell.borrow_mut();
        let buf = guard.get_or_insert_with(|| load_buffer(&BrowserStorage));
        push_bounded(buf, MAX_ENTRIES, entry);
        persist(&BrowserStorage, buf);
    });
}

/// Record an `alive`-guarded early return — but only the first one for this
/// (instance, call site) pair (see module doc). `call_site` should be a
/// short, stable literal naming the function/closure that bailed (e.g.
/// `"sync_and_upload"`, `"raf_frame"`) so the dump reads as a call-site
/// histogram, not a wall of identical lines.
pub fn record_alive_bail(instance: u32, call_site: &'static str) {
    let first_time = ALIVE_BAILS_SEEN.with(|seen| seen.borrow_mut().insert((instance, call_site)));
    if first_time {
        record(instance, "alive_bail", Some(format!("call_site={call_site}")));
    }
}

/// Format the `click_ignored` detail for the "layout still settling" reason.
/// `code_graph_view.rs`'s `on_click` is the only caller — kept here (not
/// local to that file) purely so this formatting logic can be unit-tested
/// alongside the rest of the module, the same way `record`'s formatting is
/// exercised indirectly through [`push_bounded`]/[`load_buffer`] above.
/// Includes tick/max_ticks when a driver is still attached, so the trail
/// shows how far into a settle a click was discarded — a measured 15.7s
/// local settle silently swallowed a 60-click burst with zero trace before
/// this was added, which is the exact "magic failure" Rule 14 exists to
/// close.
pub fn click_ignored_layout_settling_detail(ticks: Option<(usize, usize)>) -> String {
    match ticks {
        Some((t, max)) => format!("reason=layout_settling tick={t}/{max}"),
        None => "reason=layout_settling".to_string(),
    }
}

/// Dump the full ring buffer as a pretty-printed JSON array, oldest entry
/// first. The `architext_diagnostics()` `#[wasm_bindgen]` export in `lib.rs`
/// is the only production caller — this function itself stays plain Rust so
/// it (and everything it calls except the storage read) can be reasoned
/// about without the wasm-bindgen boundary.
pub fn dump() -> String {
    BUFFER.with(|cell| {
        let mut guard = cell.borrow_mut();
        let buf = guard.get_or_insert_with(|| load_buffer(&BrowserStorage));
        serde_json::to_string_pretty(buf).unwrap_or_else(|_| "[]".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64, instance: u32, name: &str) -> DiagEvent {
        DiagEvent { seq, ts_ms: seq as f64, instance, event: name.to_string(), detail: None }
    }

    // --- eviction -----------------------------------------------------------

    #[test]
    fn push_bounded_keeps_at_most_cap_entries() {
        let mut buf = VecDeque::new();
        for i in 0..10 {
            push_bounded(&mut buf, 3, event(i, 1, "tick"));
        }
        assert_eq!(buf.len(), 3, "buffer must never exceed the cap");
    }

    #[test]
    fn push_bounded_evicts_oldest_first() {
        let mut buf = VecDeque::new();
        for i in 0..5 {
            push_bounded(&mut buf, 3, event(i, 1, "tick"));
        }
        // Oldest-evicted-first means the survivors are the LAST 3 pushed
        // (seq 2, 3, 4), in arrival order.
        let seqs: Vec<u64> = buf.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![2, 3, 4], "eviction must drop the oldest, keep arrival order");
    }

    #[test]
    fn push_bounded_under_cap_keeps_everything() {
        let mut buf = VecDeque::new();
        for i in 0..3 {
            push_bounded(&mut buf, 300, event(i, 1, "tick"));
        }
        assert_eq!(buf.len(), 3, "nothing is evicted while under the cap");
    }

    #[test]
    fn ring_buffer_never_exceeds_the_production_cap_under_sustained_load() {
        // WHY: the concrete regression this instrumentation must never
        // cause — a stuck RAF loop retrying the SAME alive-bail call site
        // every frame must not grow the buffer unboundedly. Simulates 10,000
        // pushes (many more than a real session would ever emit through the
        // first-bail throttle) at the real MAX_ENTRIES cap.
        let mut buf = VecDeque::new();
        for i in 0..10_000u64 {
            push_bounded(&mut buf, MAX_ENTRIES, event(i, 1, "raf_frame"));
        }
        assert_eq!(buf.len(), MAX_ENTRIES);
        assert_eq!(buf.back().unwrap().seq, 9_999, "the newest entry must always survive eviction");
    }

    // --- storage tolerance ----------------------------------------------------

    /// A storage double that never touches `web_sys` — real `localStorage`
    /// panics under `cargo test`'s native host target (confirmed directly:
    /// calling `web_sys::window()` there panics with "cannot access
    /// imported statics on non-wasm targets"), so this is the only way to
    /// unit-test `load_buffer`/`persist`'s failure handling at all.
    struct FakeStorage {
        contents: RefCell<Option<String>>,
        fail_writes: bool,
    }

    impl DiagStorage for FakeStorage {
        fn read(&self) -> Option<String> {
            self.contents.borrow().clone()
        }
        fn write(&self, value: &str) -> Result<(), ()> {
            if self.fail_writes {
                return Err(());
            }
            *self.contents.borrow_mut() = Some(value.to_string());
            Ok(())
        }
    }

    #[test]
    fn load_buffer_starts_empty_when_storage_is_absent() {
        // Private browsing / storage disabled: `read()` returns `None`.
        let storage = FakeStorage { contents: RefCell::new(None), fail_writes: false };
        assert!(load_buffer(&storage).is_empty());
    }

    #[test]
    fn load_buffer_starts_empty_on_corrupt_json_instead_of_panicking() {
        let storage = FakeStorage { contents: RefCell::new(Some("not json".to_string())), fail_writes: false };
        assert!(load_buffer(&storage).is_empty(), "corrupt storage content must degrade to empty, not panic");
    }

    #[test]
    fn load_buffer_round_trips_through_persist() {
        let storage = FakeStorage { contents: RefCell::new(None), fail_writes: false };
        let mut buf = VecDeque::new();
        push_bounded(&mut buf, MAX_ENTRIES, event(1, 1, "mount"));
        push_bounded(&mut buf, MAX_ENTRIES, event(2, 1, "cleanup"));
        persist(&storage, &buf);

        let reloaded = load_buffer(&storage);
        assert_eq!(reloaded, buf, "a persisted buffer must reload identically");
    }

    #[test]
    fn persist_swallows_a_failing_write_without_panicking() {
        // Quota exceeded / private browsing mid-session: `write()` errors.
        // `persist` must not panic or propagate — this call succeeding at
        // all (returning `()`, never unwinding) IS the assertion.
        let storage = FakeStorage { contents: RefCell::new(None), fail_writes: true };
        let mut buf = VecDeque::new();
        push_bounded(&mut buf, MAX_ENTRIES, event(1, 1, "mount"));
        persist(&storage, &buf);
        assert!(storage.read().is_none(), "a failed write must leave prior storage state untouched");
    }

    // --- click_ignored detail formatting -------------------------------------

    #[test]
    fn layout_settling_detail_includes_tick_progress_when_a_driver_is_attached() {
        assert_eq!(click_ignored_layout_settling_detail(Some((37, 400))), "reason=layout_settling tick=37/400");
    }

    #[test]
    fn layout_settling_detail_degrades_gracefully_without_a_driver() {
        // The driver is dropped once `is_done()`, but `v.layout_settling` can
        // theoretically still read true for one frame in between — this must
        // not panic or fabricate tick numbers, just omit them.
        assert_eq!(click_ignored_layout_settling_detail(None), "reason=layout_settling");
    }

    // --- alive-bail throttling (pure set logic, mirrors record_alive_bail) ---

    #[test]
    fn first_time_insert_semantics_dedupe_by_instance_and_call_site() {
        let mut seen: HashSet<(u32, &'static str)> = HashSet::new();
        assert!(seen.insert((1, "sync_and_upload")), "first bail at this site for instance 1 is new");
        assert!(!seen.insert((1, "sync_and_upload")), "a repeat bail at the SAME site is suppressed");
        assert!(seen.insert((1, "full_upload")), "a DIFFERENT call site for the same instance is new");
        assert!(seen.insert((2, "sync_and_upload")), "the SAME call site for a NEW instance is new");
    }
}
