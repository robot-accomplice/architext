//! Plan D Task 3 — main-thread side of the layout-settle Web Worker: spawn
//! the worker, post one settle request, receive the reply, and the
//! app-load "warm" orchestration that ties this to `AppState`.
//!
//! `crate::force_layout` (the physics) is already worker-shaped: pure Rust,
//! no Leptos, no web-sys — it relocates unchanged into
//! `src/bin/layout_worker.rs`, a SECOND Rust/wasm binary Trunk builds from
//! this same crate (see `index.html`'s `data-type="worker"` link). This file
//! is the glue around that: it owns the wire format both sides must agree
//! on, so it is `use`d from BOTH the main thread (this module) and the
//! worker binary (`architext_viewer::layout_worker_client::{decode_request,
//! encode_result}`), keeping the field names in exactly one place.
//!
//! MESSAGE CONTRACT (flat, numeric, transferable — no `GraphModel`,
//! `CodeGraph`, or any string):
//! - Request (main → worker): `nodeCount`, `seedHi`/`seedLo` (a `u64` split
//!   into two `u32`s — JS numbers can't hold the view's full 64-bit seed
//!   losslessly), `maxTicks`, the `ForceConfig` scalars, and `edges` — a
//!   flattened `(from, to)` `Uint32Array`.
//! - Reply (worker → main): `positions` — a flattened `(x, y)`
//!   `Float32Array` — and `ticksRun`.
//!
//! Both typed arrays travel as `postMessage` transferables (their
//! `ArrayBuffer`, not `SharedArrayBuffer` — `architext-serve` sends no
//! COOP/COEP headers and this must not need them).
//!
//! READY HANDSHAKE: a worker's `main()` yields to the JS event loop while
//! its wasm loads, so any `postMessage` sent before it registers its
//! `onmessage` listener is silently dropped, not queued (confirmed against
//! Trunk's own `examples/webworker`). The worker posts an empty ready-ping
//! first; this side only posts the real request once that ping arrives.
//!
//! CANCELLATION (Task 3 Step 3 — "no racing writers"): the view never lets a
//! main-thread settle and this worker's settle both run for the same
//! `(sha, tree, tier)`. `WarmCancelHandle::cancel` (`Worker::terminate`) lets
//! the view stop an in-flight warm the instant it decides to settle that
//! answer itself — see `components/code_graph_view.rs`'s cache-miss branch.
use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, Float32Array, Object, Reflect, Uint32Array};
use leptos::*;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{MessageEvent, Worker};

use crate::code_graph_view_model::{build_graph, Tier, LAYOUT_SEED};
use crate::data::models::CodeGraph;
use crate::diagnostics;
use crate::force_layout::ForceConfig;
use crate::layout_cache::LayoutKey;
use crate::state::AppState;

/// Where Trunk emits the worker's loader shim — see `index.html`. Worker
/// output is deliberately never content-hashed by Trunk (a worker's own
/// script must be reachable at a name known ahead of the build), so this
/// path is stable across rebuilds. An absolute path matches every other
/// same-origin reference in this crate (`data/fetch.rs`'s `/api/*`,
/// `/data/*`) — the app assumes root-mounted serving throughout.
const WORKER_LOADER_URL: &str = "/layout_worker_loader.js";

// --- wire format: field names -----------------------------------------------

const FIELD_NODE_COUNT: &str = "nodeCount";
const FIELD_SEED_HI: &str = "seedHi";
const FIELD_SEED_LO: &str = "seedLo";
const FIELD_MAX_TICKS: &str = "maxTicks";
const FIELD_THETA: &str = "theta";
const FIELD_K: &str = "k";
const FIELD_GRAVITY: &str = "gravity";
const FIELD_CONVERGENCE_EPS: &str = "convergenceEps";
const FIELD_EDGES: &str = "edges";
const FIELD_POSITIONS: &str = "positions";
const FIELD_TICKS_RUN: &str = "ticksRun";

// --- wire format: pure helpers (unit-testable natively, no JS engine needed) -

fn split_seed(seed: u64) -> (u32, u32) {
    ((seed >> 32) as u32, seed as u32)
}

fn join_seed(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn flatten_edges(edges: &[(usize, usize)]) -> Vec<u32> {
    let mut out = Vec::with_capacity(edges.len() * 2);
    for &(a, b) in edges {
        out.push(a as u32);
        out.push(b as u32);
    }
    out
}

fn unflatten_edges(flat: &[u32]) -> Vec<(usize, usize)> {
    flat.chunks_exact(2).map(|p| (p[0] as usize, p[1] as usize)).collect()
}

fn flatten_positions(positions: &[(f32, f32)]) -> Vec<f32> {
    let mut out = Vec::with_capacity(positions.len() * 2);
    for &(x, y) in positions {
        out.push(x);
        out.push(y);
    }
    out
}

fn unflatten_positions(flat: &[f32]) -> Vec<(f32, f32)> {
    flat.chunks_exact(2).map(|p| (p[0], p[1])).collect()
}

fn get_u32(obj: &JsValue, key: &str) -> Result<u32, JsValue> {
    Reflect::get(obj, &key.into())?
        .as_f64()
        .map(|v| v as u32)
        .ok_or_else(|| JsValue::from_str(&format!("missing/invalid numeric field: {key}")))
}

fn get_f64(obj: &JsValue, key: &str) -> Result<f64, JsValue> {
    Reflect::get(obj, &key.into())?
        .as_f64()
        .ok_or_else(|| JsValue::from_str(&format!("missing/invalid numeric field: {key}")))
}

/// Build the outbound settle request plus its transfer list (the `edges`
/// buffer). Main-thread only.
fn encode_request(
    node_count: u32,
    edges: &[(usize, usize)],
    seed: u64,
    cfg: &ForceConfig,
) -> Result<(Object, Array), JsValue> {
    let edges_arr = Uint32Array::from(flatten_edges(edges).as_slice());
    let (seed_hi, seed_lo) = split_seed(seed);

    let msg = Object::new();
    Reflect::set(&msg, &FIELD_NODE_COUNT.into(), &node_count.into())?;
    Reflect::set(&msg, &FIELD_SEED_HI.into(), &seed_hi.into())?;
    Reflect::set(&msg, &FIELD_SEED_LO.into(), &seed_lo.into())?;
    Reflect::set(&msg, &FIELD_MAX_TICKS.into(), &(cfg.max_ticks as u32).into())?;
    Reflect::set(&msg, &FIELD_THETA.into(), &cfg.theta.into())?;
    Reflect::set(&msg, &FIELD_K.into(), &cfg.k.into())?;
    Reflect::set(&msg, &FIELD_GRAVITY.into(), &cfg.gravity.into())?;
    Reflect::set(&msg, &FIELD_CONVERGENCE_EPS.into(), &cfg.convergence_eps.into())?;
    Reflect::set(&msg, &FIELD_EDGES.into(), &edges_arr)?;

    let transfer = Array::new();
    transfer.push(&edges_arr.buffer());
    Ok((msg, transfer))
}

/// `(node_count, edges, seed, config)` — the decoded settle request.
type SettleRequest = (u32, Vec<(usize, usize)>, u64, ForceConfig);

/// Parse an inbound settle request. `pub` — the worker entry point
/// (`src/bin/layout_worker.rs`, a separate Cargo binary target in this same
/// crate) calls this so the two sides share one wire format instead of a
/// second, potentially drifting copy of the field names.
pub fn decode_request(data: &JsValue) -> Result<SettleRequest, JsValue> {
    let node_count = get_u32(data, FIELD_NODE_COUNT)?;
    let seed = join_seed(get_u32(data, FIELD_SEED_HI)?, get_u32(data, FIELD_SEED_LO)?);
    let max_ticks = get_u32(data, FIELD_MAX_TICKS)? as usize;
    let cfg = ForceConfig {
        theta: get_f64(data, FIELD_THETA)?,
        k: get_f64(data, FIELD_K)?,
        gravity: get_f64(data, FIELD_GRAVITY)?,
        max_ticks,
        convergence_eps: get_f64(data, FIELD_CONVERGENCE_EPS)?,
    };
    let edges_arr: Uint32Array = Reflect::get(data, &FIELD_EDGES.into())?.dyn_into()?;
    let edges = unflatten_edges(&edges_arr.to_vec());
    Ok((node_count, edges, seed, cfg))
}

/// Build the settle reply plus its transfer list (the `positions` buffer).
/// `pub` — called from the worker entry point.
pub fn encode_result(positions: &[(f32, f32)], ticks_run: usize) -> Result<(Object, Array), JsValue> {
    let pos_arr = Float32Array::from(flatten_positions(positions).as_slice());
    let msg = Object::new();
    Reflect::set(&msg, &FIELD_POSITIONS.into(), &pos_arr)?;
    Reflect::set(&msg, &FIELD_TICKS_RUN.into(), &(ticks_run as u32).into())?;
    let transfer = Array::new();
    transfer.push(&pos_arr.buffer());
    Ok((msg, transfer))
}

/// Parse the worker's reply. Main-thread only.
fn decode_result(data: &JsValue) -> Result<(Vec<(f32, f32)>, usize), JsValue> {
    let ticks_run = get_u32(data, FIELD_TICKS_RUN)? as usize;
    let pos_arr: Float32Array = Reflect::get(data, &FIELD_POSITIONS.into())?.dyn_into()?;
    Ok((unflatten_positions(&pos_arr.to_vec()), ticks_run))
}

// --- spawn/receive -----------------------------------------------------------

/// One-shot completion slot: `take()`n and called at most once, from
/// whichever of `onmessage`/`onerror` fires first (see `fire`).
type SettleCallback = Rc<RefCell<Option<Box<dyn FnOnce(WorkerOutcome)>>>>;

/// Outcome of one worker settle request.
pub enum WorkerOutcome {
    Settled { positions: Vec<(f32, f32)>, ticks_run: usize },
    /// Covers every failure shape (unsupported, blocked, a malformed reply):
    /// they all collapse to the SAME fallback — the caller proceeds with
    /// today's main-thread progressive settle (Task 3 Step 4). A missing or
    /// broken worker degrades to current behaviour, never to a broken view.
    Failed,
}

/// Spawn the layout worker and hand it exactly one settle request.
///
/// Returns the `Worker` handle on a successful synchronous spawn — the
/// caller can `.terminate()` it later via `WarmCancelHandle` to cancel a
/// warm that's no longer needed. Returns `None` if the worker could not even
/// be constructed; `on_settled` has ALREADY been called with `Failed` in
/// that case, so the caller has nothing further to do.
pub fn spawn_settle(
    node_count: u32,
    edges: &[(usize, usize)],
    seed: u64,
    cfg: &ForceConfig,
    on_settled: impl FnOnce(WorkerOutcome) + 'static,
) -> Option<Worker> {
    let callback: SettleCallback = Rc::new(RefCell::new(Some(Box::new(on_settled))));

    let worker = match Worker::new(WORKER_LOADER_URL) {
        Ok(w) => w,
        Err(e) => {
            leptos::logging::log!("[layout-worker-client] Worker::new FAILED: {e:?}");
            fire(&callback, WorkerOutcome::Failed);
            return None;
        }
    };

    let (request_msg, request_transfer) = match encode_request(node_count, edges, seed, cfg) {
        Ok(r) => r,
        Err(e) => {
            leptos::logging::log!("[layout-worker-client] encode_request FAILED: {e:?}");
            fire(&callback, WorkerOutcome::Failed);
            return None;
        }
    };

    // First message in is the ready-ping (see module docs); only then is it
    // safe to post the real request. `ready_seen` distinguishes the two.
    let ready_seen = Rc::new(RefCell::new(false));
    let onmessage_worker = worker.clone();
    let onmessage_callback = callback.clone();
    let onmessage = Closure::wrap(Box::new(move |ev: MessageEvent| {
        if !*ready_seen.borrow() {
            *ready_seen.borrow_mut() = true;
            if let Err(e) = onmessage_worker.post_message_with_transfer(&request_msg, &request_transfer) {
                leptos::logging::log!("[layout-worker-client] post_message FAILED: {e:?}");
                fire(&onmessage_callback, WorkerOutcome::Failed);
            }
            return;
        }
        match decode_result(&ev.data()) {
            Ok((positions, ticks_run)) => {
                fire(&onmessage_callback, WorkerOutcome::Settled { positions, ticks_run })
            }
            Err(e) => {
                leptos::logging::log!("[layout-worker-client] decode_result FAILED: {e:?}");
                fire(&onmessage_callback, WorkerOutcome::Failed);
            }
        }
    }) as Box<dyn FnMut(MessageEvent)>);
    worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    // Leaked deliberately (same one-shot pattern as the RAF closures in
    // `components/code_graph_view.rs`) — this is a single app-load warm, not
    // a per-frame allocation, so the leak is bounded and negligible.
    onmessage.forget();

    let onerror_callback = callback;
    let onerror = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
        leptos::logging::log!("[layout-worker-client] worker onerror fired");
        fire(&onerror_callback, WorkerOutcome::Failed);
    }) as Box<dyn FnMut(web_sys::Event)>);
    worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    Some(worker)
}

fn fire(slot: &SettleCallback, outcome: WorkerOutcome) {
    if let Some(cb) = slot.borrow_mut().take() {
        cb(outcome);
    }
}

// --- app-load warm + AppState glue ------------------------------------------

/// A live worker's cancel handle. Wraps `web_sys::Worker` (itself cheap to
/// clone — a JS reference) so `CodeGraphWarm::Running` can be cloned freely.
#[derive(Debug, Clone)]
pub struct WarmCancelHandle(Worker);

impl WarmCancelHandle {
    /// Stop the worker immediately. Used when the view is about to compute
    /// the SAME answer itself (Task 3 Step 3): terminating first guarantees
    /// there is only ever one eventual writer of this layout's cache entry
    /// — never two settles racing to finish.
    pub fn cancel(&self) {
        self.0.terminate();
    }
}

/// Status of the app-load background warm (Task 3 Step 3). Lives on
/// `AppState`, not scoped to one Code Graph view instance: the warm starts
/// at app load, before any Code Graph view exists, and the view is torn
/// down and rebuilt on every mode switch (see `components/code_graph_view
/// .rs`'s module docs on render-loop cancellation) — only `AppState`
/// outlives that. `Running` is only ever produced for the function tier
/// (see `warm_function_tier`) — it does not carry a `Tier` because there is
/// only ever one warm target.
#[derive(Debug, Clone)]
pub enum CodeGraphWarm {
    /// No computable code graph at load, or the warm hasn't been kicked off.
    Idle,
    /// A worker is settling `(sha, tree)`'s function tier right now.
    Running { sha: String, tree: String, cancel: WarmCancelHandle },
    /// Settled, cancelled, or failed — nothing left to do or cancel. Any
    /// settled positions are already in `AppState::layout_cache`.
    Finished,
}

/// Guards `warm_function_tier` to at most one real spawn per wasm instance.
///
/// Defense in depth for any future path that could call this more than once
/// within the SAME running app (e.g. a refactor of `App()`'s load branch) —
/// spawning a worker is not free like the other one-shot app-load actions
/// (a CLI-version fetch, say): it is a real OS-level thread doing real CPU
/// work, so a second spawn genuinely doubles the background cost for zero
/// benefit (both would settle the exact same deterministic answer). wasm32
/// CSR is single-threaded, so a plain `AtomicBool` swap is sufficient with
/// no locking needed.
///
/// NOTE: this cannot guard across two SEPARATE wasm instances — each gets
/// its own linear memory and thus its own fresh `false`. The live-verify
/// sandbox for this task instantiates the whole module twice per navigation
/// (confirmed via a temporary `main()`-entry diagnostic, not assumed — see
/// the Task 3 report), which this guard cannot see or prevent. That is an
/// environment/tooling characteristic of the sandbox's preview harness, not
/// a defect introduced here: it would double-fire every other one-shot
/// app-load action too (the CLI-version fetch, the mutation-token fetch,
/// the live-reload SSE connect), none of which this task touches.
static WARM_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Kick off the app-load warm for the function tier (Task 3 Step 3): once
/// `load_architecture_data()` resolves with a computable code graph, settle
/// its expensive tier (functions; modules at 205 nodes settle fast enough on
/// demand) in a worker so a later Code Graph entry is very likely a
/// `layout_cache` hit before the user ever gets there.
///
/// Fire-and-forget: the caller (`App()` in `lib.rs`) invokes this and moves
/// on immediately — the result (or failure) lands on `state.layout_cache` /
/// `state.code_graph_warm` whenever the worker is ready, never blocking app
/// startup.
pub fn warm_function_tier(state: AppState, cg: &CodeGraph) {
    if WARM_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return; // already warming (or warmed) this page load — see WARM_STARTED's doc
    }
    if !cg.computable {
        return;
    }
    let graph = build_graph(cg, Tier::Functions);
    let node_count = graph.node_count();
    if node_count == 0 {
        return; // nothing to warm
    }
    let key = LayoutKey::new(cg.sha.clone(), cg.tree.clone(), Tier::Functions);
    let cfg = ForceConfig::default();

    // Diagnostics module doc item 4 ("whether the layout came from
    // cache/worker/local"): this is the ONLY producer of the "worker" source
    // value — `code_graph_view.rs` only ever settles locally or reuses a
    // cache entry this warm may have written. `NO_INSTANCE` (0) because this
    // runs at app load, before any `CodeGraphViewCanvas` exists to own an
    // instance id.
    let warm_t0 = js_sys::Date::now();
    diagnostics::record(
        diagnostics::NO_INSTANCE,
        "layout_settle_start",
        Some(format!("source=worker tier=Functions nodes={node_count}")),
    );

    let worker = spawn_settle(node_count as u32, &graph.layout_edges, LAYOUT_SEED, &cfg, move |outcome| {
        match outcome {
            WorkerOutcome::Settled { positions, ticks_run } => {
                leptos::logging::log!(
                    "[layout-worker-client] warm settled: nodes={node_count} ticks={ticks_run}"
                );
                diagnostics::record(
                    diagnostics::NO_INSTANCE,
                    "layout_settle_end",
                    Some(format!(
                        "source=worker tier=Functions ticks={ticks_run} elapsed_ms={:.0}",
                        js_sys::Date::now() - warm_t0
                    )),
                );
                state.layout_cache.update(|c| c.put(key, positions));
            }
            WorkerOutcome::Failed => {
                leptos::logging::log!(
                    "[layout-worker-client] warm did not complete — a later entry falls back \
                     to main-thread ticking"
                );
            }
        }
        state.code_graph_warm.set(CodeGraphWarm::Finished);
    });

    if let Some(w) = worker {
        state.code_graph_warm.set(CodeGraphWarm::Running {
            sha: cg.sha.clone(),
            tree: cg.tree.clone(),
            cancel: WarmCancelHandle(w),
        });
    }
    // else: `spawn_settle` already invoked the closure above with `Failed`,
    // which already set `Finished` — nothing more to do.
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- pure wire-format round-trips ---------------------------------------
    //
    // The `Reflect`/`Object`/typed-array calls in `encode_request` etc. are
    // wasm-bindgen JS imports and cannot run outside a real wasm+JS engine —
    // confirmed directly: a bare `js_sys::Object::new()` call under `cargo
    // test` (native) panics with "cannot call wasm-bindgen imported
    // functions on non-wasm targets". So THESE tests cover the part of the
    // wire format that IS pure Rust — the seed split/join and the
    // flatten/unflatten transforms — at the real scale (≥1000 nodes,
    // 17,814 at the real corpus). The JS `Reflect`/`Object` boundary itself
    // is exercised live, in a browser (see the Task 3 report).

    #[test]
    fn seed_splits_and_rejoins_exactly_for_the_views_fixed_seed() {
        let (hi, lo) = split_seed(LAYOUT_SEED);
        assert_eq!(join_seed(hi, lo), LAYOUT_SEED, "hi/lo split must reassemble to the exact seed");
    }

    #[test]
    fn seed_round_trips_for_boundary_values() {
        for seed in [0u64, u64::MAX, 1, 1u64 << 32, (1u64 << 32) - 1] {
            let (hi, lo) = split_seed(seed);
            assert_eq!(join_seed(hi, lo), seed, "seed {seed} must round-trip");
        }
    }

    #[test]
    fn edges_flatten_and_unflatten_round_trip_at_1000_interconnected_nodes() {
        let (_, edges) = crate::code_graph_graph::tests_support::interconnected(1000, 3);
        let flat = flatten_edges(&edges);
        assert_eq!(flat.len(), edges.len() * 2, "flattened length must be exactly 2x the pair count");
        assert_eq!(unflatten_edges(&flat), edges, "edges must round-trip exactly");
    }

    #[test]
    fn positions_flatten_and_unflatten_round_trip_at_1000_settled_nodes() {
        // WHY 1000 settled (not synthetic) positions: proves the transform
        // survives real f32 settle output (fractional coordinates, negative
        // values), not just round tripping integers.
        let (n, edges) = crate::code_graph_graph::tests_support::interconnected(1000, 3);
        let cfg = ForceConfig::default();
        let positions: Vec<(f32, f32)> = crate::force_layout::simulate(n, &edges, LAYOUT_SEED, &cfg)
            .positions
            .iter()
            .map(|&(x, y)| (x as f32, y as f32))
            .collect();
        let flat = flatten_positions(&positions);
        assert_eq!(flat.len(), positions.len() * 2);
        assert_eq!(unflatten_positions(&flat), positions, "positions must round-trip exactly");
    }
}
