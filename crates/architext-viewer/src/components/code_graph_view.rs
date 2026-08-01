//! The code-graph WebGL view (Plan C Task 4): canvas setup, pan/zoom, click
//! hit-testing, the control chrome (tier toggle, reachability + edge-kind
//! culling filters, animation modes, play/step, depth scrub), and the
//! animation timer — lifted from the proven spike
//! (`docs/superpowers/spike-source/spike-c-webgl/code_graph_gl.rs`) onto the
//! Task 3 `gl::Renderer` plumbing and the Task 2 pure model
//! (`crate::code_graph_view_model`). Zero JavaScript anywhere.
//!
//! Required behaviour changes from the spike (maintainer decisions):
//! - FILTERS CULL, never fade: excluded nodes/edges are not uploaded to the
//!   GPU at all — a filter change re-uploads (`upload_static` + dynamic).
//! - FILTERS DEFAULT ON: `FilterState::default()` (prod-reachable only), so
//!   the 17k-node tier never opens as a hairball.
//! - AUTO-PLAY: entering the function tier starts the roots animation, with
//!   an obvious pause and stop & reset in the chrome.
//! - "Showing N of M" renders whenever anything is culled.
//! - Selection stays PANEL-LOCAL: a code-graph index is never written to
//!   `AppState::selected_node` (different id-space — the inspector resolves
//!   that against `data.nodes`). Task 6 adds a one-way MIRROR: the selected
//!   node's Magma id (plus its tier) is copied to
//!   `AppState::selected_code_graph_node` so the inspector can render detail,
//!   via the same `sync_and_upload`/`full_upload` mirrors as the footer label.
//!
//! RENDER-LOOP CANCELLATION (carried forward verbatim from the spike):
//! `canvas_panel.rs`'s outer mode-render closure can re-evaluate more than
//! once per `set_mode()` call, tearing down and rebuilding this component
//! each time. Leptos disposes the torn-down instance's signals; a
//! still-in-flight callback (an effect re-run, a timer tick, or the
//! continuous RAF loop) that fires after disposal would otherwise panic
//! (`OwnerDisposed`) and TRAP THE WHOLE WASM INSTANCE — every signal write
//! anywhere in the app freezes. So `on_cleanup` flips `alive`, and every
//! callback below checks `alive` before touching a Leptos signal. The RAF
//! closure is intentionally leaked (`mem::forget`); the flipped `alive`
//! makes its next frame return early WITHOUT re-scheduling, killing the loop.
//!
//! The `alive` guard has one hole it cannot cover: leptos 0.6 `create_effect`
//! defers the effect's FIRST run to a microtask wrapped in
//! `with_owner(owner).unwrap()` — if the teardown above disposes the owner
//! before that microtask runs, the unwrap panics (`OwnerDisposed`) BEFORE the
//! effect body (and its `alive` check) executes. Symbolicated from a debug
//! wasm build in Task 8: repeated `RuntimeError: unreachable` traps on Code
//! Graph mode entry, one per torn-down instance per effect. So every effect
//! in this component is a `create_render_effect`, whose first run is
//! SYNCHRONOUS — the owner is provably alive inside the component body, and
//! later re-runs only come from live signal subscriptions (disposed effects
//! are unsubscribed). The first run early-returns on `canvas_ref.get()`
//! being `None` either way, so behaviour is unchanged.
//!
//! PROGRESSIVE LAYOUT (Plan C Task 5): the Barnes-Hut layout no longer runs
//! as one blocking `simulate` call (measured 6.5–27 s of frozen main thread
//! at 17,561 nodes). The tier effect only SEEDS the layout
//! (`LayoutDriver::new`, tick-0 circle) and uploads it immediately — first
//! paint is one frame. The continuous RAF loop then spends up to
//! `LAYOUT_FRAME_BUDGET_MS` per frame running ticks, re-uploading positions
//! so the user watches the graph settle. No new scheduling is introduced:
//! the layout slice rides the existing alive-guarded RAF loop, so disposal
//! kills it with the same `alive` flip. Determinism is preserved because
//! slicing changes when ticks run, never what they compute (see
//! `code_graph_layout.rs`); click hit-testing is gated while settling because
//! the quadtree still covers pre-settle positions.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::html::Canvas;
use leptos::*;
use wasm_bindgen::JsCast;

use crate::code_graph_graph::FilterState;
use crate::code_graph_provenance::{
    discloses_executed_target_code, dynamic_edge_explanation, fidelity_method_description,
};
use crate::code_graph_layout::LayoutDriver;
use crate::code_graph_view_model::{
    build_graph, fit_camera, should_autoplay, AnimMode, Tier, ViewState, LAYOUT_SEED,
};
use crate::data::models::CodeGraph;
use crate::diagnostics;
use crate::force_layout::{ForceConfig, QuadTree};
use crate::gl::renderer::Renderer;
use crate::layout_cache::LayoutKey;
use crate::layout_worker_client::CodeGraphWarm;
use crate::state::{use_app_state, CodeGraphSelection};

/// Per-frame millisecond budget for progressive layout ticks (Task 5). At
/// 17,561 nodes one tick costs ~15-20 ms, so the budget usually buys one
/// tick per frame there — the frame rate dips during the settle but every
/// frame still yields to input/paint, and the `step_within` contract
/// guarantees at least one tick so a slow tick can't starve progress.
/// Small tiers blow through their whole layout in the first frame or two.
const LAYOUT_FRAME_BUDGET_MS: f64 = 12.0;

/// How long (ms, measured against the RAF timestamp clock) a frame may go
/// missing before the `visibilitychange` handler force-resumes the loop.
/// A healthy foreground tab paints every ~16 ms; 500 ms is >30x that, so it
/// only fires for a genuinely stalled chain (backgrounded tab, minimized
/// window, OS focus loss — browsers throttle or fully suspend RAF callbacks
/// for hidden documents, confirmed directly: a bare `requestAnimationFrame`
/// loop with zero app code fires 0 times in 5 s while `document.hidden` is
/// true). The resumed frame can race a callback the browser was *also*
/// about to redeliver on its own, briefly doubling the RAF chain until the
/// component next unmounts — an accepted, harmless-in-practice tradeoff
/// against the alternative of a canvas frozen forever.
const STALL_RESUME_THRESHOLD_MS: f64 = 500.0;

/// `prefers-reduced-motion: reduce` — read once per tier (re)seed and stashed
/// on `ViewState`, never polled per frame. Motion-reduced users fall back to
/// the pre-rework "instant reveal" (see `code_graph_view_model::Wavefront`'s
/// `growth` field and `ViewState::advance_animation`'s cadence choice)
/// instead of the progressive edge-growth animation.
fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
        .map(|mql| mql.matches())
        .unwrap_or(false)
}

/// Live render-pipeline progress facts for the staged progress panel —
/// covers the two stages of tier entry that are genuinely observable from
/// this component:
///   1. "Building graph model" (`build_graph`) — a single synchronous call,
///      so it has no sub-progress; `build_ms` is its real measured wall-clock
///      cost and it reads as DONE the instant this struct exists (the call
///      already returned before `Some(RenderProgress)` is ever constructed).
///   2. "Laying out graph" — the ~400-tick force settle, mirrored from the
///      layout driver's tick/time state every ticked RAF frame (unchanged
///      from the prior single-stage instrumentation).
///
/// `None` while idle (tier not yet settling, or already settled) — the panel
/// only exists while stage 2 has real work left to show.
///
/// A third candidate stage, loading/parsing `code-graph.json`, is NOT
/// tracked here: that fetch+deserialize happens in `data::fetch::
/// load_architecture_data` before the viewer even mounts (gated by the
/// app-level `LoadingScreen`, not this component), so it is not observable
/// from inside the code-graph view. A fourth candidate, GPU upload/first
/// paint, is folded into stage 2 rather than given its own row: the upload
/// re-runs every settle tick as part of the existing frame-budget slice
/// (`full_upload` in the RAF loop below), so it is never a separate,
/// independently-timed phase — inventing a bar for it would just duplicate
/// stage 2's.
#[derive(Clone, Copy, PartialEq)]
struct RenderProgress {
    /// Real measured cost of the `build_graph` call for this tier — always
    /// "done" by the time this struct is observed (see above).
    build_ms: f64,
    ticks: usize,
    max_ticks: usize,
    elapsed_ms: f64,
    node_count: usize,
    edge_count: usize,
}

/// Outer surface: mirrors the 4-surface shape of the code-graph panel this
/// replaces (no document / unreadable / refusal / real graph).
#[component]
pub fn CodeGraphView() -> impl IntoView {
    let state = use_app_state();
    view! {
        <div class="code-graph-view">
            {move || {
                let data = state.data.get();
                match &data.code_graph {
                    None => view! {
                        <div class="code-graph-view__empty">
                            <h2>"No code graph yet"</h2>
                            <p>"This project has no " <code>"code-graph.json"</code> " registered under "
                               <code>"manifest.files.codeGraph"</code> "."</p>
                        </div>
                    }.into_view(),
                    Some(Err(err)) => view! {
                        <div class="code-graph-view__empty">
                            <h2>"Code graph could not be read"</h2>
                            <p>{err.to_string()}</p>
                        </div>
                    }.into_view(),
                    Some(Ok(cg)) if !cg.computable => {
                        let reason = cg.not_computable_reason.clone()
                            .unwrap_or_else(|| "no reason given".to_string());
                        view! {
                            <div class="code-graph-view__empty">
                                <h2>"No graph available"</h2>
                                <p>{reason}</p>
                            </div>
                        }.into_view()
                    }
                    Some(Ok(cg)) => view! { <CodeGraphViewCanvas cg=cg.clone()/> }.into_view(),
                }
            }}
        </div>
    }
}

// Everything `sync_and_upload`/`full_upload` mirror onto their Leptos
// signals after touching `vs`, snapshotted OUT of the RefCell borrow. Module
// scope (not nested in `CodeGraphViewCanvas` below) so the regression test
// at the bottom of this file can reach it directly.
struct SelectionMirror {
    selected_label: Option<(String, u32)>,
    anim_current_depth: i32,
    anim_max_depth: i32,
    counts: (usize, usize, usize, usize),
    next: Option<CodeGraphSelection>,
}

// THE FIX for the release-blocking crash: borrow `vs` just long enough to
// copy out everything the two upload paths mirror, then hand back OWNED
// data — never a reference borrowed from `vs`. Both call sites used to read
// straight out of `vs.borrow().as_ref()` and keep that borrow alive across
// `state.set_selected_code_graph_node(next)`, which runs the "TRUE CLEAR"
// effect (further down `CodeGraphViewCanvas`) SYNCHRONOUSLY — and that
// effect does its own `vs.borrow_mut()`. A `borrow_mut()` reentering while a
// `borrow()` is still live panics ("RefCell already borrowed"), and a panic
// inside wasm traps the WHOLE instance (see the module doc's RENDER-LOOP
// CANCELLATION section) — every signal write app-wide freezes, which is
// exactly the user's report of "changing options at the top clears the
// graph and never rerenders it." Returning owned data makes the bug
// structurally impossible to reintroduce: the `Ref` this function takes is
// dropped at the end of the statement below (the same tail-expression
// pattern the RAF frame's "build inside the borrow, set after it's
// released" comment relies on), which is BEFORE this function returns to
// its caller — so there is no borrow left for the caller to still be
// holding. Regression coverage: `refcell_discipline_tests` at the bottom of
// this file.
fn snapshot_selection_mirror(
    vs: &Rc<RefCell<Option<ViewState>>>,
    tier: Tier,
    selection_id: &impl Fn(Tier, usize) -> Option<String>,
) -> Option<SelectionMirror> {
    // `.map()`, not `if let ... else`: same tail-expression temporary-drop
    // timing either way (the `Ref` from `vs.borrow()` is dropped at the end
    // of this statement, before the function returns — proven by
    // `refcell_discipline_tests::snapshot_selection_mirror_releases_the_borrow_before_returning`
    // below), and clippy's `manual_map` flags the longer form as a warning.
    vs.borrow().as_ref().map(|v| SelectionMirror {
        selected_label: v.selected.map(|i| (v.graph.labels[i].clone(), v.graph.degree[i])),
        anim_current_depth: v.anim_current_depth,
        anim_max_depth: v.anim_max_depth,
        counts: (
            v.cull.nodes.len(),
            v.graph.node_count(),
            v.cull.edges.len(),
            v.graph.directed_edges.len(),
        ),
        next: v.selected.and_then(|i| selection_id(tier, i)).map(|id| CodeGraphSelection { tier, id }),
    })
}

#[component]
fn CodeGraphViewCanvas(cg: CodeGraph) -> impl IntoView {
    let state = use_app_state();
    // Rule 14 RCA instrumentation (see `diagnostics` module doc): a
    // process-wide id for THIS component instance. `canvas_panel.rs`'s outer
    // mode-render closure can tear this component down and rebuild it more
    // than once per `set_mode()` — every event below carries this id so a
    // diagnostics dump can tell "this instance was torn down and a fresh one
    // replaced it" apart from "this instance died and nothing replaced it".
    let diag_instance = diagnostics::next_instance_id();
    diagnostics::record(diag_instance, "mount", Some(format!("sha={} tree={}", cg.sha, cg.tree)));
    let canvas_ref = create_node_ref::<Canvas>();
    let tier = create_rw_signal(Tier::Functions);
    let status = create_rw_signal(String::new());
    let fps_label = create_rw_signal(String::from("— fps"));
    let gl_error = create_rw_signal::<Option<String>>(None);
    let selected_label = create_rw_signal::<Option<(String, u32)>>(None);
    // (visible nodes, total nodes, visible edges, total edges) — the
    // "Showing N of M" notice renders whenever anything is culled.
    let counts = create_rw_signal((0usize, 0usize, 0usize, 0usize));
    let filter = create_rw_signal(FilterState::default());
    // Staged progress panel: `None` while idle, `Some` and refreshed every
    // ticked frame while a layout is settling — the primary busy signal
    // (centred panel, per-stage bars), status/fps stay as secondary detail
    // in the toolbar.
    let render_progress = create_rw_signal::<Option<RenderProgress>>(None);
    let anim_mode_sig = create_rw_signal(AnimMode::Off);
    let anim_depth_sig = create_rw_signal(0i32);
    let anim_max_sig = create_rw_signal(0i32);
    let anim_playing_sig = create_rw_signal(false);

    // GPU + view state live OUTSIDE Leptos signals (see ViewState docs).
    let gpu: Rc<RefCell<Option<Renderer>>> = Rc::new(RefCell::new(None));
    let vs: Rc<RefCell<Option<ViewState>>> = Rc::new(RefCell::new(None));
    // Progressive layout (Task 5): the seeded, still-ticking driver for the
    // current tier. The RAF loop slices it; `None` once settled. Plain
    // `Cell`s for the instrumentation/camera bookkeeping — nothing reads
    // them reactively (same rationale as the drag `Cell`s below).
    let layout: Rc<RefCell<Option<LayoutDriver>>> = Rc::new(RefCell::new(None));
    // Plan D Task 2 cache, keyed on (sha, tree, tier) — lives on `AppState`
    // (Task 3), not this component: the app-load background warm
    // (`layout_worker_client::warm_function_tier`) writes into it before any
    // Code Graph view exists, and this component is torn down and rebuilt on
    // every mode switch (see the module docs on render-loop cancellation), so
    // a component-local cache would start cold on every re-entry. `cg_sha`/
    // `cg_tree` are cloned out of the loaded envelope now because `cg` itself
    // is moved into the tier-entry effect below, but the RAF loop (defined
    // after it) also needs them to `put` on settle completion.
    let cg_sha = cg.sha.clone();
    let cg_tree = cg.tree.clone();
    // Provenance surface (Item 1): the facts the "how this map was made"
    // affordance and the fidelity-modulated dynamic-edge explanation need,
    // cloned out for the same reason as `cg_sha`/`cg_tree` above — `cg`
    // itself is moved into the tier-entry effect below.
    let cg_generator = cg.generator.clone();
    let cg_executed_target_code = cg.executed_target_code;
    // Computed once from `cg.fidelity` (fixed for this component instance's
    // life) so nothing downstream needs to hold onto `cg.fidelity` itself —
    // both are `&'static str`, so they're `Copy` and freely reusable across
    // the reactive closures below.
    let dynamic_edge_title = dynamic_edge_explanation(&cg.fidelity);
    let cg_method_description = fidelity_method_description(&cg.fidelity);
    let provenance_open = create_rw_signal(false);
    let layout_t0: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let first_paint_logged: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let user_moved_camera: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    // RAF resilience: the timestamp (RAF clock) of the last frame that
    // actually ran. The `visibilitychange` handler below compares against
    // this to tell a genuinely stalled loop from a merely-slow one.
    let last_frame_at: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    // Animation-rework: the RAF timestamp of the last frame that actually
    // advanced the wavefront while playing — `0.0` is the "start the dt
    // clock fresh next frame" sentinel (same pattern as `last_frame_at`),
    // set whenever `play()` (re)starts so the very first frame after a press
    // never jumps by however long the animation sat paused/stopped.
    let last_anim_frame_at: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    // Non-reactive mirror of `tier`'s current value, kept for `on_cleanup`
    // below: cleanup runs AS this component's reactive scope is disposed, so
    // reading a Leptos signal there — even via `get_untracked` — is exactly
    // the disposed-owner hazard the module doc warns about. This plain
    // `Cell` (same pattern as `user_moved_camera` etc. above) gives cleanup
    // instrumentation the tier without touching the `tier` signal itself.
    let tier_mirror: Rc<Cell<Tier>> = Rc::new(Cell::new(Tier::Functions));

    let alive: Rc<RefCell<bool>> = Rc::new(RefCell::new(true));
    on_cleanup({
        let alive = alive.clone();
        let vs = vs.clone();
        let layout = layout.clone();
        let tier_mirror = tier_mirror.clone();
        move || {
            *alive.borrow_mut() = false;
            // Cleanup is the pivotal instrumentation event (module doc item
            // 2): record it with whatever context is cheaply available.
            // `vs`/`layout` are plain `Rc<RefCell<..>>`, not Leptos signals —
            // safe to read regardless of disposal order — and `tier_mirror`
            // exists precisely so tier is available here too.
            //
            // `try_borrow`, never `borrow`: the recorded trail shows cleanup
            // fires WHILE a settle is in flight every single time (the
            // teardown storm on mode entry disposes each instance ~15 ms after
            // it starts its layout), so the RAF tick loop may well be holding
            // `layout.borrow_mut()` right now. A panic here would trap the
            // whole wasm instance — see the `try_borrow` note in `on_click`.
            // Unknown beats dead: `None` renders as "unknown" in the detail.
            let fmt = |b: Option<bool>| b.map_or("unknown".to_string(), |v| v.to_string());
            let layout_settling = fmt(layout.try_borrow().ok().map(|l| l.is_some()));
            let selection_held = fmt(
                vs.try_borrow().ok().map(|v| v.as_ref().map(|v| v.selected.is_some()).unwrap_or(false)),
            );
            diagnostics::record(
                diag_instance,
                "cleanup",
                Some(format!(
                    "tier={:?} layout_settling={layout_settling} selection_held={selection_held}",
                    tier_mirror.get()
                )),
            );
        }
    });

    // Map a full-graph index back to its Magma id for the inspector mirror
    // (Task 6). Positional correspondence: `build_graph` pushes one node per
    // function/module in collection order, so index `i` IS `functions[i]` /
    // `modules[i]` — the same correspondence the footer label already trusts
    // via `graph.labels[i]`.
    let selection_id = {
        let cg = cg.clone();
        move |t: Tier, i: usize| -> Option<String> {
            match t {
                Tier::Functions => cg.functions.as_ref()?.get(i).map(|f| f.id.clone()),
                Tier::Modules => cg.modules.as_ref()?.get(i).map(|m| m.id.clone()),
            }
        }
    };

    // Format the selection-mirror instrumentation detail (module doc item
    // 5): set-to-Some / cleared. Shared by `sync_and_upload` and
    // `full_upload` below — both write the SAME mirror through the SAME
    // equality-guarded pattern, so one formatter keeps the wording from
    // drifting between the two copies.
    fn selection_mirror_detail(next: &Option<CodeGraphSelection>) -> String {
        match next {
            Some(sel) => format!("set id={}", sel.id),
            None => "cleared".to_string(),
        }
    }

    // DYNAMIC upload only (selection/animation/scrub path) + mirror the
    // display facts into signals. Called imperatively after every mutation
    // site instead of on a tracked dependency — the RefCell is the truth,
    // the signals are its display mirror.
    let sync_and_upload = {
        let gpu = gpu.clone();
        let vs = vs.clone();
        let alive = alive.clone();
        let selection_id = selection_id.clone();
        move || {
            if let (Some(g), Some(v)) = (gpu.borrow().as_ref(), vs.borrow().as_ref()) {
                g.upload_dynamic(&v.node_state(), &v.edge_state());
            }
            if !*alive.borrow() {
                // Module doc item 3 — the "SKIPPED-because-not-alive" case
                // for the selection mirror: without this, a torn-down
                // instance's mirror write silently no-ops and the inspector
                // just... stops updating, with nothing recorded anywhere.
                diagnostics::record_alive_bail(diag_instance, "sync_and_upload");
                return;
            }
            // `snapshot_selection_mirror` releases its `vs` borrow before
            // returning (see its doc) — the fix for the crash where holding
            // `vs.borrow()` across `state.set_selected_code_graph_node` below
            // double-borrowed the RefCell and trapped the wasm instance.
            if let Some(snap) = snapshot_selection_mirror(&vs, tier.get_untracked(), &selection_id) {
                selected_label.set(snap.selected_label);
                anim_depth_sig.set(snap.anim_current_depth);
                anim_max_sig.set(snap.anim_max_depth);
                // Inspector mirror (Task 6): None when nothing is selected, so
                // a cleared canvas selection clears the inspector detail too.
                // The equality guard matters: this closure also runs per
                // layout-settle frame, and an unconditional `set` would
                // re-render the inspector body every frame with the same value.
                if state.selected_code_graph_node.get_untracked() != snap.next {
                    // Module doc item 5: the mirror actually changing is rare
                    // (gated by the equality check above), so this never
                    // becomes a per-frame flood — no throttling needed here.
                    diagnostics::record(
                        diag_instance,
                        "selection_mirror",
                        Some(selection_mirror_detail(&snap.next)),
                    );
                    state.set_selected_code_graph_node(snap.next);
                }
            }
        }
    };

    // FULL re-upload (filter/cull path): the visible node/edge sets changed,
    // so the STATIC buffers are rebuilt for the culled sets, then the dynamic
    // state on top. Culled items are never uploaded — no fade, no haze.
    let full_upload = {
        let gpu = gpu.clone();
        let vs = vs.clone();
        let alive = alive.clone();
        let selection_id = selection_id.clone();
        move || {
            {
                let mut gpu_guard = gpu.borrow_mut();
                let vs_guard = vs.borrow();
                if let (Some(g), Some(v)) = (gpu_guard.as_mut(), vs_guard.as_ref()) {
                    let (node_pos_radius, edge_endpoints) = v.static_interleaves();
                    g.upload_static(&node_pos_radius, &edge_endpoints);
                    g.upload_dynamic(&v.node_state(), &v.edge_state());
                }
            }
            if !*alive.borrow() {
                // Hot path: this runs every layout-settle RAF tick, so
                // `record_alive_bail` throttles to the first bail per
                // instance (see its doc) rather than flooding the buffer.
                diagnostics::record_alive_bail(diag_instance, "full_upload");
                return;
            }
            // Same fix as `sync_and_upload` above: `snapshot_selection_mirror`
            // releases the `vs` borrow before returning, so the signal writes
            // below — most of all `state.set_selected_code_graph_node`, whose
            // reentrant `vs.borrow_mut()` (the "TRUE CLEAR" effect) is what
            // trapped the wasm instance — can never race a still-held borrow.
            if let Some(snap) = snapshot_selection_mirror(&vs, tier.get_untracked(), &selection_id) {
                counts.set(snap.counts);
                selected_label.set(snap.selected_label);
                // Depth-chrome mirror: `full_upload` is now the path
                // `set_anim_mode`/`set_depth`/a hop-boundary crossing take
                // (the cull set changes with the wavefront — see their call
                // sites), so it must mirror the depth label/slider itself
                // rather than leaving that to `sync_and_upload` alone, or
                // "depth N / M" and the scrub slider go stale on every mode
                // change and step/scrub.
                anim_depth_sig.set(snap.anim_current_depth);
                anim_max_sig.set(snap.anim_max_depth);
                // Inspector mirror (Task 6): runs on the cull/tier paths too,
                // so a filter that culls the selection (or a tier switch,
                // which rebuilds `vs` with `selected: None`) also clears the
                // inspector detail. Same equality guard as `sync_and_upload`:
                // this runs per layout-settle frame.
                if state.selected_code_graph_node.get_untracked() != snap.next {
                    // Same equality guard as `sync_and_upload` bounds this to
                    // real changes, not one entry per settle frame.
                    diagnostics::record(
                        diag_instance,
                        "selection_mirror",
                        Some(selection_mirror_detail(&snap.next)),
                    );
                    state.set_selected_code_graph_node(snap.next);
                }
            }
        }
    };

    // --- Animation mode / depth / play-pause / step ---
    //
    // Progressive edge-draw rework: playback no longer runs its own
    // `gloo_timers::callback::Interval` ticking whole depth layers on/off —
    // it rides the ALREADY-RUNNING continuous RAF loop below, which each
    // frame calls `ViewState::advance_animation` (pure timing in
    // `code_graph_view_model::advance_hop`) to move `anim_hop_progress`
    // toward the next depth at the readable-trace pace. `play`/`pause` here
    // are just the `anim_playing_sig` flag the RAF loop gates on, plus the
    // "restart from 0 if already at the end" convenience the button had
    // before. `set_anim_mode`/`set_depth` still do a FULL upload (not just
    // `sync_and_upload`): the wavefront restriction now changes which
    // nodes/edges are culled from the upload (see `ViewState::recompute_cull`
    // and `cull`'s `Wavefront` doc), so the STATIC buffers may need rebuilding
    // on every mode/depth change, not just the dynamic per-instance state.
    let set_anim_mode = {
        let vs = vs.clone();
        let full_upload = full_upload.clone();
        let alive = alive.clone();
        move |mode: AnimMode| {
            if !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "set_anim_mode");
                return;
            }
            anim_playing_sig.set(false);
            anim_mode_sig.set(mode);
            if let Some(v) = vs.borrow_mut().as_mut() {
                v.anim_mode = mode;
                v.recompute_bfs(); // also re-culls to match (see its doc)
            }
            full_upload();
        }
    };
    let set_depth = {
        let vs = vs.clone();
        let full_upload = full_upload.clone();
        let alive = alive.clone();
        move |d: i32| {
            if let Some(v) = vs.borrow_mut().as_mut() {
                v.set_anim_depth(d); // clamps, resets hop_progress, re-culls
            }
            if !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "set_depth");
                return;
            }
            full_upload();
        }
    };
    let step = {
        let vs = vs.clone();
        let set_depth = set_depth.clone();
        move |delta: i32| {
            // Block-scoped borrow so the guard is released BEFORE `set_depth`
            // (which needs its own `borrow_mut`) runs — holding it across the
            // call would double-borrow the RefCell and panic.
            let next = {
                let guard = vs.borrow();
                let Some(v) = guard.as_ref() else {
                    // Same defensive/theoretically-unreachable shape as
                    // `on_click`'s `no_view_state` bail — reusing
                    // `click_ignored` (not a new event name) keeps the
                    // vocabulary small; `call_site` disambiguates it from
                    // the canvas click path.
                    diagnostics::record(
                        diag_instance,
                        "click_ignored",
                        Some("reason=no_view_state call_site=step".to_string()),
                    );
                    return;
                };
                (v.anim_current_depth + delta).clamp(0, v.anim_max_depth)
            };
            set_depth(next);
        }
    };
    let play = {
        let vs = vs.clone();
        let full_upload = full_upload.clone();
        let last_anim_frame_at = last_anim_frame_at.clone();
        let alive = alive.clone();
        move || {
            if !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "play");
                return;
            }
            // Pressing play after the wavefront reached the end RESTARTS it
            // from depth 0 rather than freezing on the final frame.
            let mut restarted = false;
            if let Some(v) = vs.borrow_mut().as_mut() {
                if v.anim_current_depth >= v.anim_max_depth {
                    v.set_anim_depth(0);
                    restarted = true;
                }
            }
            // Next RAF frame starts the playback dt clock fresh — otherwise
            // its `now - last_anim_frame_at` would include however long the
            // animation sat paused/stopped, and `advance_hop` (correctly)
            // treats that as a huge dt and fast-forwards straight to the end.
            last_anim_frame_at.set(0.0);
            anim_playing_sig.set(true);
            if restarted {
                full_upload();
            }
        }
    };
    let pause = {
        let alive = alive.clone();
        move || {
            if !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "pause");
                return;
            }
            anim_playing_sig.set(false);
        }
    };

    // (Re)build + SEED the layout on tier change (Task 5: the layout itself
    // now ticks progressively in the RAF loop below — this effect must stay
    // fast, it is the time-to-first-paint path). Layout runs ONCE per tier
    // over the FULL graph (positions stay stable across filter changes —
    // culling only changes what gets uploaded, never where anything sits).
    {
        let gpu = gpu.clone();
        let vs = vs.clone();
        let alive = alive.clone();
        let full_upload = full_upload.clone();
        let layout = layout.clone();
        let cg_sha = cg_sha.clone();
        let cg_tree = cg_tree.clone();
        let layout_t0 = layout_t0.clone();
        let first_paint_logged = first_paint_logged.clone();
        let user_moved_camera = user_moved_camera.clone();
        let set_anim_mode = set_anim_mode.clone();
        let play = play.clone();
        let tier_mirror = tier_mirror.clone();
        create_render_effect(move |_| {
            let t = tier.get();
            tier_mirror.set(t); // keep the cleanup-time mirror current
            let Some(canvas) = canvas_ref.get() else { return };
            if !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "tier_effect");
                return;
            }

            // Lazily create the WebGL2 renderer exactly once (the canvas DOM
            // node persists across tier switches; a second get_context would
            // return the same context). Failure is an explicit surface, never
            // a blank canvas.
            if gpu.borrow().is_none() {
                match Renderer::new(&canvas) {
                    Ok(r) => *gpu.borrow_mut() = Some(r),
                    Err(e) => {
                        leptos::logging::log!("[code-graph-view] WebGL2 init FAILED: {e}");
                        gl_error.set(Some(e));
                        return;
                    }
                }
            }

            anim_playing_sig.set(false); // stop the previous tier's animation

            // Real, measured cost of stage 1 ("Building graph model") — see
            // `RenderProgress` docs for why this is the whole stage (no
            // sub-progress: `build_graph` is one synchronous call) and why
            // it always reads DONE by the time the panel can show it.
            let perf = web_sys::window().and_then(|w| w.performance());
            let t_build0 = perf.as_ref().map(|p| p.now()).unwrap_or(0.0);
            let graph = build_graph(&cg, t);
            let build_ms = perf.as_ref().map(|p| p.now()).unwrap_or(0.0) - t_build0;
            let n = graph.node_count();
            let edge_count = graph.directed_edges.len();
            let (w, h) = (canvas.width() as f32, canvas.height() as f32);

            // Plan D Task 2: a settled layout for this exact (sha, tree,
            // tier) has already been computed once this session — reuse it
            // instead of re-running ~400 ticks to arrive at the provably same
            // answer (the layout is a pure deterministic function of
            // (edges, seed, tick_count); see `layout_cache` module docs).
            // Task 3: this also catches an app-load worker warm that already
            // finished — its result lands here via `state.layout_cache`.
            let cache_key = LayoutKey::new(cg_sha.clone(), cg_tree.clone(), t);
            let cache_hit = state.layout_cache.with_untracked(|c| c.get(&cache_key).map(|p| p.to_vec()));

            user_moved_camera.set(false);
            first_paint_logged.set(false);
            layout_t0.set(perf.as_ref().map(|p| p.now()).unwrap_or(0.0));

            if let Some(positions) = cache_hit {
                // HIT: skip the settle entirely — no driver, no progress
                // panel, no re-seed, no animation restart. Straight to
                // interactive, camera fit over the cached positions.
                let (zoom, pan_x, pan_y) = fit_camera(&positions, &graph.radius, w, h);
                let tree = QuadTree::from_positions_f32(&positions);
                let mut new_vs =
                    ViewState::new(graph, positions, tree, pan_x, pan_y, zoom, prefers_reduced_motion());
                new_vs.layout_settling = false;
                *vs.borrow_mut() = Some(new_vs);
                *layout.borrow_mut() = None; // nothing to tick — already settled
                leptos::logging::log!(
                    "[code-graph-view] layout cache HIT: nodes={n} edges={edge_count} tier={t:?}"
                );
                // Module doc item 4: a cache hit has no settle PROCESS (zero
                // ticks, no elapsed time to speak of) — record just the "end"
                // fact rather than fabricating a "start" for work that never
                // ran, same rationale as the instant-done branch below.
                diagnostics::record(
                    diag_instance,
                    "layout_settle_end",
                    Some(format!("source=cache tier={t:?} nodes={n} edges={edge_count}")),
                );
                full_upload();
                status.set(format!("{n} nodes / {edge_count} edges"));
                render_progress.set(None);
                filter.set(FilterState::default());
                // A cache hit still honours auto-play on the function tier —
                // there is no settle event to hang it off, so trigger it here
                // directly instead of deferring to the RAF loop.
                if should_autoplay(t) {
                    set_anim_mode(AnimMode::Roots);
                    play();
                } else {
                    set_anim_mode(AnimMode::Off);
                }
            } else {
                // MISS: settle as today.
                //
                // Task 3 "no racing writers": if the app-load worker warm is
                // still computing THIS exact (sha, tree) function-tier
                // answer, cancel it before starting a redundant main-thread
                // settle — determinism means the two would compute the same
                // bit-identical positions, but there must be only one
                // eventual writer of this cache entry, not two settles
                // racing to finish. `Running` is only ever produced for the
                // function tier, so this is gated to it explicitly: an
                // unrelated Modules-tier miss must NOT cancel a Functions
                // warm still in flight.
                if t == Tier::Functions {
                    if let CodeGraphWarm::Running { sha, tree, cancel } =
                        state.code_graph_warm.get_untracked()
                    {
                        if sha == cg_sha && tree == cg_tree {
                            cancel.cancel();
                            state.code_graph_warm.set(CodeGraphWarm::Finished);
                            leptos::logging::log!(
                                "[code-graph-view] cancelled in-flight warm — settling on the main thread instead"
                            );
                        }
                    }
                }
                let driver = LayoutDriver::new(n, &graph.layout_edges, LAYOUT_SEED, &ForceConfig::default());
                let max_ticks = driver.max_ticks();
                // Tick-0 positions (the seeded circle) upload IMMEDIATELY so
                // the first frame paints a real graph, not a spinner.
                let positions = driver.positions_f32();
                let (zoom, pan_x, pan_y) = fit_camera(&positions, &graph.radius, w, h);
                let settling = !driver.is_done();
                // Captured before `driver` moves into `layout` below — only
                // used by the "already done at seed" branch's instant
                // `layout_settle_end` (module doc item 4).
                let ticks_at_seed = driver.ticks_run();

                // `vs` MUST be replaced BEFORE any of the signal writes below:
                // each `.set()` synchronously re-runs the filter effect (which
                // re-uploads, sized from `vs`). Writing the signals first let
                // that reentrant upload fire while `vs` still held the
                // PREVIOUS tier's counts against buffers just reallocated to
                // the NEW tier's size — a `bufferSubData` overflow (the
                // spike's bug, kept fixed). The initial hit-test tree covers
                // the seeded positions; the RAF loop replaces it with the
                // settled tree.
                let mut new_vs = ViewState::new(
                    graph,
                    positions,
                    driver.hit_tree(),
                    pan_x,
                    pan_y,
                    zoom,
                    prefers_reduced_motion(),
                );
                new_vs.layout_settling = settling;
                *vs.borrow_mut() = Some(new_vs);
                *layout.borrow_mut() = Some(driver);
                leptos::logging::log!(
                    "[code-graph-view] layout seeded: nodes={n} edges={edge_count} settling={settling}"
                );
                full_upload();

                if settling {
                    diagnostics::record(
                        diag_instance,
                        "layout_settle_start",
                        Some(format!("source=local tier={t:?} nodes={n} edges={edge_count} max_ticks={max_ticks}")),
                    );
                    status.set(format!(
                        "{n} nodes / {edge_count} edges — layout settling… (click-to-select deferred until settled)"
                    ));
                    render_progress.set(Some(RenderProgress {
                        build_ms,
                        ticks: 0,
                        max_ticks,
                        elapsed_ms: 0.0,
                        node_count: n,
                        edge_count,
                    }));
                } else {
                    // Tiny tiers can be done at tick 0 (no ticks needed) —
                    // same "no fabricated start for a process that never
                    // ran" rationale as the cache-hit branch above.
                    diagnostics::record(
                        diag_instance,
                        "layout_settle_end",
                        Some(format!("source=local tier={t:?} ticks={ticks_at_seed} elapsed_ms=0")),
                    );
                    status.set(format!("{n} nodes / {edge_count} edges"));
                    render_progress.set(None);
                }
                filter.set(FilterState::default());
                // AUTO-PLAY ON OPEN is deferred to the settle (RAF loop
                // below): starting the wavefront while positions churn would
                // sweep the animation across a graph that is still moving.
                set_anim_mode(AnimMode::Off);
            }
        });
    }

    // Continuous RAF redraw loop — always running so pan/zoom/animation fps
    // is honestly measurable rather than only-repaint-on-change. Started
    // once on mount; cancelled on unmount via `alive` (see module docs).
    // The Task 5 progressive layout slice rides THIS loop (no new
    // scheduling), so disposal cancels it with the same `alive` flip.
    {
        let gpu = gpu.clone();
        let vs = vs.clone();
        let alive = alive.clone();
        let layout = layout.clone();
        let cg_sha = cg_sha.clone();
        let cg_tree = cg_tree.clone();
        let full_upload = full_upload.clone();
        let sync_and_upload = sync_and_upload.clone();
        let set_anim_mode = set_anim_mode.clone();
        let play = play.clone();
        let layout_t0 = layout_t0.clone();
        let first_paint_logged = first_paint_logged.clone();
        let user_moved_camera = user_moved_camera.clone();
        let last_frame_at = last_frame_at.clone();
        let last_anim_frame_at = last_anim_frame_at.clone();
        let perf = web_sys::window().and_then(|w| w.performance());
        type FrameCb = wasm_bindgen::closure::Closure<dyn FnMut(f64)>;
        let frame_cb: Rc<RefCell<Option<FrameCb>>> = Rc::new(RefCell::new(None));
        let frame_times: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::with_capacity(120)));
        let frame_cb2 = frame_cb.clone();
        // A third handle to the same leaked closure, for the `visibilitychange`
        // listener below to re-arm the loop if it ever goes quiet (see there).
        let frame_cb_vis = frame_cb.clone();
        let alive_vis = alive.clone();
        let last_frame_at_vis = last_frame_at.clone();
        *frame_cb.borrow_mut() = Some(wasm_bindgen::closure::Closure::wrap(Box::new(move |now: f64| {
            if !*alive.borrow() {
                // Hottest path in the component (up to 60/s) — throttled to
                // the first bail per instance, see `record_alive_bail`'s doc.
                // This is the exact call site the original, undiagnosable
                // failure needed: an `alive == false` instance whose RAF
                // loop kept getting scheduled would show up here as ONE
                // entry instead of silence.
                diagnostics::record_alive_bail(diag_instance, "raf_frame");
                return;
            }
            last_frame_at.set(now);
            // --- Progressive layout slice (Task 5) ---
            // Spend up to LAYOUT_FRAME_BUDGET_MS ticking the seeded layout,
            // then hand the frame back to input/paint. Positions are
            // re-uploaded every ticked frame so the user WATCHES the graph
            // settle; the status line and `render_progress` mirror tick
            // progress.
            let mut ticked = false;
            let mut settled_ticks: Option<usize> = None;
            // Plan D Task 2: the positions to `put` into the layout cache
            // once this settle completes — cloned only on the settling
            // frame (once per tier, not once per tick).
            let mut positions_for_cache: Option<Vec<(f32, f32)>> = None;
            {
                let mut drv_guard = layout.borrow_mut();
                if let Some(d) = drv_guard.as_mut() {
                    if !d.is_done() {
                        ticked = true;
                        let done =
                            d.step_within(LAYOUT_FRAME_BUDGET_MS, || {
                                perf.as_ref().map(|p| p.now()).unwrap_or(0.0)
                            });
                        let positions = d.positions_f32();
                        if done {
                            positions_for_cache = Some(positions.clone());
                        }
                        let (ticks, max) = (d.ticks_run(), d.max_ticks());
                        // Never stomp a camera the user already moved.
                        let refit = !user_moved_camera.get();
                        // Build the status line INSIDE the borrow but set the
                        // signal AFTER it is released — a signal write runs
                        // dependent effects synchronously, and one of those
                        // borrowing `vs` here would double-borrow the RefCell.
                        let frame_facts = if let Some(v) = vs.borrow_mut().as_mut() {
                            v.positions = positions;
                            // Re-fit the camera on EVERY ticked frame, not
                            // just tick 0 and the final settle: the sim's
                            // footprint moves a great deal across the whole
                            // settle (measured ~19x contraction at 17,561
                            // nodes — see `code_graph_layout` tests), so a
                            // camera fit only once at seed frames the WRONG
                            // extent for nearly the entire settle, reading
                            // as a blank canvas around a point the graph
                            // has already shrunk away from. Continuous
                            // refit keeps it legibly visible throughout.
                            if refit {
                                if let Some(canvas) = canvas_ref.get_untracked() {
                                    let (zoom, pan_x, pan_y) = fit_camera(
                                        &v.positions,
                                        &v.graph.radius,
                                        canvas.width() as f32,
                                        canvas.height() as f32,
                                    );
                                    v.zoom = zoom;
                                    v.pan_x = pan_x;
                                    v.pan_y = pan_y;
                                }
                            }
                            Some((
                                format!(
                                    "{} nodes / {} edges — settling layout (tick {ticks}/{max}) — click-to-select deferred",
                                    v.graph.node_count(),
                                    v.graph.directed_edges.len()
                                ),
                                v.graph.node_count(),
                                v.graph.directed_edges.len(),
                            ))
                        } else {
                            None
                        };
                        if *alive.borrow() {
                            if let Some((line, node_count, edge_count)) = frame_facts {
                                status.set(line);
                                // Stage 1's measured cost doesn't change tick
                                // to tick — carry it forward from whatever
                                // the tier effect seeded so this per-tick
                                // update doesn't need its own copy of it.
                                let build_ms =
                                    render_progress.get_untracked().map(|p| p.build_ms).unwrap_or(0.0);
                                render_progress.set(Some(RenderProgress {
                                    build_ms,
                                    ticks,
                                    max_ticks: max,
                                    elapsed_ms: perf.as_ref().map(|p| p.now()).unwrap_or(0.0)
                                        - layout_t0.get(),
                                    node_count,
                                    edge_count,
                                }));
                            }
                        }
                        if done {
                            settled_ticks = Some(ticks);
                            let tree = d.hit_tree();
                            if let Some(v) = vs.borrow_mut().as_mut() {
                                v.tree = tree;
                                v.layout_settling = false;
                            }
                        }
                    }
                    if d.is_done() {
                        *drv_guard = None; // settled (or empty): drop the driver
                    }
                }
            }
            if ticked {
                // Static buffers carry positions, so every slice re-uploads
                // them (the cull sets are unchanged — same nodes, new spots).
                full_upload();
            }
            if let Some(positions) = positions_for_cache {
                // Plan D Task 2: remember this settle so the NEXT entry into
                // this (sha, tree, tier) is a cache hit instead of another
                // ~400-tick recompute of the same, provably identical answer.
                let key = LayoutKey::new(cg_sha.clone(), cg_tree.clone(), tier.get_untracked());
                state.layout_cache.update(|c| c.put(key, positions));
            }
            if let Some(ticks) = settled_ticks {
                if *alive.borrow() {
                    let (n, e) = vs
                        .borrow()
                        .as_ref()
                        .map(|v| (v.graph.node_count(), v.graph.directed_edges.len()))
                        .unwrap_or((0, 0));
                    status.set(format!("{n} nodes / {e} edges — {ticks} ticks"));
                    render_progress.set(None);
                    let elapsed = perf.as_ref().map(|p| p.now() - layout_t0.get()).unwrap_or(0.0);
                    leptos::logging::log!(
                        "[code-graph-view] layout settled: ticks={ticks} time-to-settled={elapsed:.0}ms"
                    );
                    diagnostics::record(
                        diag_instance,
                        "layout_settle_end",
                        Some(format!(
                            "source=local tier={:?} ticks={ticks} elapsed_ms={elapsed:.0}",
                            tier.get_untracked()
                        )),
                    );
                    // AUTO-PLAY ON OPEN (deferred from the tier effect until
                    // the positions stop moving): the function tier starts
                    // the roots animation automatically; the chrome offers
                    // an obvious pause and stop & reset so it is never
                    // imposed. Modules stay static (no per-module root flags
                    // to sweep from).
                    if should_autoplay(tier.get_untracked()) {
                        set_anim_mode(AnimMode::Roots);
                        play();
                    }
                }
            }
            // --- Animation playback slice (progressive edge-draw rework) ---
            // Rides this SAME RAF loop: while `anim_playing_sig` is set, each
            // frame advances the wavefront's `hop_progress` by the elapsed
            // wall-clock time (pure timing in `ViewState::advance_animation`
            // / `code_graph_view_model::advance_hop`, paced by
            // `HOP_DURATION_MS` — the readable-trace 1-2s/hop). A hop-boundary
            // crossing changes which nodes/edges `cull` admits (see its
            // `Wavefront` doc) and needs a full re-upload; a same-hop tick
            // only moves the in-flight edges' GPU `progress` floats, so the
            // cheaper DYNAMIC-only `sync_and_upload` suffices — no per-frame
            // STATIC rebuild, matching the "no per-edge CPU work beyond the
            // existing dynamic-upload path" constraint.
            if anim_playing_sig.get_untracked() {
                let dt = if last_anim_frame_at.get() > 0.0 { now - last_anim_frame_at.get() } else { 0.0 };
                last_anim_frame_at.set(now);
                let mut outcome: Option<(bool, bool)> = None;
                if let Some(v) = vs.borrow_mut().as_mut() {
                    if v.anim_mode != AnimMode::Off {
                        outcome = Some(v.advance_animation(dt));
                    }
                }
                if let Some((boundary_crossed, finished)) = outcome {
                    if *alive.borrow() {
                        if boundary_crossed {
                            full_upload();
                        } else {
                            sync_and_upload();
                        }
                        if finished {
                            anim_playing_sig.set(false);
                        }
                    }
                }
            }
            {
                let mut ft = frame_times.borrow_mut();
                ft.push(now);
                if ft.len() > 120 {
                    ft.remove(0);
                }
                if ft.len() >= 2 {
                    let span = ft[ft.len() - 1] - ft[0];
                    if span > 0.0 {
                        let fps = (ft.len() as f64 - 1.0) / (span / 1000.0);
                        fps_label.set(format!("{fps:.0} fps"));
                    }
                }
            }
            let mut drew = false;
            if let (Some(canvas), Some(g), Some(v)) =
                (canvas_ref.get_untracked(), gpu.borrow().as_ref(), vs.borrow().as_ref())
            {
                g.draw(&canvas, v.pan_x, v.pan_y, v.zoom);
                drew = true;
            }
            if drew && !first_paint_logged.get() && layout_t0.get() > 0.0 {
                first_paint_logged.set(true);
                let elapsed = perf.as_ref().map(|p| p.now() - layout_t0.get()).unwrap_or(0.0);
                leptos::logging::log!("[code-graph-view] time-to-first-paint={elapsed:.0}ms");
            }
            if let Some(w) = web_sys::window() {
                if let Some(cb) = frame_cb2.borrow().as_ref() {
                    let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
                }
            }
        }) as Box<dyn FnMut(f64)>));
        if let Some(w) = web_sys::window() {
            if let Some(cb) = frame_cb.borrow().as_ref() {
                let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }
        std::mem::forget(frame_cb); // intentional — cancelled via `alive`

        // RAF resilience: browsers throttle or fully suspend
        // `requestAnimationFrame` callbacks for a hidden/backgrounded
        // document (confirmed directly — a bare RAF loop with no app code
        // fires zero times in 5 s while `document.hidden` is true). There is
        // otherwise no path back to painting if that happens mid-settle:
        // the loop is the only thing that ever calls `draw`. On regaining
        // visibility, force one resumed frame if the last one is stale
        // beyond `STALL_RESUME_THRESHOLD_MS` — cheap insurance against a
        // canvas stuck frozen forever, at the cost of a possible harmless
        // one-off double-schedule if the browser was already about to
        // redeliver the pending callback on its own.
        let perf_vis = web_sys::window().and_then(|w| w.performance());
        let vis_cb = wasm_bindgen::closure::Closure::wrap(Box::new(move |_evt: web_sys::Event| {
            if !*alive_vis.borrow() {
                // Fires at most a handful of times per session (a user
                // switching tabs) — no per-frame throttling concern here,
                // but `record_alive_bail` is still the right call: uniform
                // mechanism, and a torn-down instance's listener staying
                // registered at all is itself worth knowing about once.
                diagnostics::record_alive_bail(diag_instance, "visibilitychange");
                return;
            }
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
            let hidden = doc.hidden();
            // Module doc item 6: both directions of the transition, not just
            // the resume path below — this is the only place either is
            // observable.
            diagnostics::record(diag_instance, "visibilitychange", Some(format!("hidden={hidden}")));
            if hidden {
                return; // only act on the hidden -> visible transition
            }
            let now = perf_vis.as_ref().map(|p| p.now()).unwrap_or(0.0);
            if now - last_frame_at_vis.get() > STALL_RESUME_THRESHOLD_MS {
                let stalled_ms = now - last_frame_at_vis.get();
                leptos::logging::log!("[code-graph-view] RAF stalled — resuming on visibilitychange");
                diagnostics::record(
                    diag_instance,
                    "raf_stall_resume",
                    Some(format!("stalled_ms={stalled_ms:.0}")),
                );
                if let (Some(w), Some(cb)) = (web_sys::window(), frame_cb_vis.borrow().as_ref()) {
                    let _ = w.request_animation_frame(cb.as_ref().unchecked_ref());
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc.add_event_listener_with_callback("visibilitychange", vis_cb.as_ref().unchecked_ref());
        }
        std::mem::forget(vis_cb); // leaked like `frame_cb` — `alive` guards it
    }

    // --- Interaction: drag = pan, wheel = zoom, click (no drag) = select ---
    // Plain `Cell`s, not Leptos signals: pure imperative gesture bookkeeping
    // that nothing reads reactively — keeping them out of the reactive graph
    // sidesteps the disposed-owner hazard `alive` guards against elsewhere.
    let dragging: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let moved: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let last: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));

    let on_mouse_down = {
        let (dragging, moved, last) = (dragging.clone(), moved.clone(), last.clone());
        move |ev: ev::MouseEvent| {
            dragging.set(true);
            moved.set(false);
            last.set((ev.client_x() as f64, ev.client_y() as f64));
        }
    };
    let on_mouse_move = {
        let vs = vs.clone();
        let user_moved_camera = user_moved_camera.clone();
        let (dragging, moved, last) = (dragging.clone(), moved.clone(), last.clone());
        move |ev: ev::MouseEvent| {
            if !dragging.get() {
                return;
            }
            let (lx, ly) = last.get();
            let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
            let (dx, dy) = (cx - lx, cy - ly);
            if dx.abs() > 2.0 || dy.abs() > 2.0 {
                moved.set(true);
                user_moved_camera.set(true);
            }
            last.set((cx, cy));
            if let (Some(canvas), Some(v)) = (canvas_ref.get_untracked(), vs.borrow_mut().as_mut()) {
                // CSS px → backing-store px (the canvas is 1600x1000
                // intrinsic, stretched to its CSS box).
                let rect = canvas.get_bounding_client_rect();
                let scale_x = canvas.width() as f64 / rect.width();
                let scale_y = canvas.height() as f64 / rect.height();
                v.pan_x += (dx * scale_x) as f32;
                v.pan_y += (dy * scale_y) as f32;
            }
        }
    };
    let end_drag = { let dragging = dragging.clone(); move |_: ev::MouseEvent| dragging.set(false) };

    let on_wheel = {
        let vs = vs.clone();
        let user_moved_camera = user_moved_camera.clone();
        move |ev: ev::WheelEvent| {
            ev.prevent_default();
            user_moved_camera.set(true);
            let factor = if ev.delta_y() < 0.0 { 1.15 } else { 1.0 / 1.15 };
            if let Some(v) = vs.borrow_mut().as_mut() {
                v.zoom = (v.zoom * factor as f32).clamp(0.01, 8.0);
            }
        }
    };

    let on_click = {
        let vs = vs.clone();
        let layout = layout.clone();
        let sync_and_upload = sync_and_upload.clone();
        let full_upload = full_upload.clone();
        let set_anim_mode = set_anim_mode.clone();
        move |ev: ev::MouseEvent| {
            if moved.get() {
                // Module doc / Rule 14 follow-up: this and the other
                // `click_ignored` sites below are exactly the silent
                // discards that made a real settling-window click burst
                // (60 clicks, zero trace) indistinguishable from a dead
                // view. User-driven, not per-frame, so — unlike
                // `record_alive_bail` — every occurrence is recorded, not
                // just the first: a run of these IS the diagnosis.
                diagnostics::record(diag_instance, "click_ignored", Some("reason=drag".to_string()));
                return; // the mouseup that ends a drag is not a click
            }
            let Some(canvas) = canvas_ref.get_untracked() else {
                // Defensive: the canvas element owns this handler, so this
                // is not expected to be reachable in practice — recorded
                // anyway since it's another silent early return in this
                // function and costs nothing to cover.
                diagnostics::record(diag_instance, "click_ignored", Some("reason=no_canvas".to_string()));
                return;
            };
            let rect = canvas.get_bounding_client_rect();
            let scale_x = canvas.width() as f64 / rect.width();
            let scale_y = canvas.height() as f64 / rect.height();
            let sx = (ev.client_x() as f64 - rect.left()) * scale_x;
            let sy = (ev.client_y() as f64 - rect.top()) * scale_y;

            // Resolve the hit against `cull.filter_visible` — FILTER-culled
            // only, never narrowed by an in-progress animation wavefront (see
            // `Cull`'s doc). An animation-culled node is a real, on-screen-
            // adjacent node the wavefront simply hasn't drawn yet and must
            // stay clickable; only a node the user's own filters hide is
            // rejected here — that is the defect-1 fix.
            let (hit, was_animating) = {
                let guard = vs.borrow();
                let Some(v) = guard.as_ref() else {
                    // `vs` is only `None` in the brief window before the
                    // tier effect's synchronous first run — defensive, like
                    // `no_canvas` above.
                    diagnostics::record(diag_instance, "click_ignored", Some("reason=no_view_state".to_string()));
                    return;
                };
                if v.layout_settling {
                    // The painted positions refresh every frame but the
                    // hit-test tree still covers pre-settle positions — a
                    // hit now would select the WRONG node. Selection opens
                    // when the layout settles (status line says so). This is
                    // the call site the maintainer traced the 60-click burst
                    // to — record how far into the settle it landed.
                    // `try_borrow`, never `borrow`: this runs DURING a settle,
                    // which is exactly when the RAF tick loop holds
                    // `layout.borrow_mut()`. A plain `borrow()` here panicked
                    // ("RefCell already mutably borrowed"), and a panic in wasm
                    // traps the WHOLE instance — every signal write in the app
                    // freezes while the DOM stays on screen looking alive, which
                    // is precisely the un-diagnosable failure this module exists
                    // to prevent. Instrumentation must never be able to take the
                    // app down: if the driver is busy, record without the ticks.
                    let ticks = layout
                        .try_borrow()
                        .ok()
                        .and_then(|l| l.as_ref().map(|d| (d.ticks_run(), d.max_ticks())));
                    diagnostics::record(
                        diag_instance,
                        "click_ignored",
                        Some(diagnostics::click_ignored_layout_settling_detail(ticks)),
                    );
                    return;
                }
                let gx = ((sx as f32) - v.pan_x) / v.zoom;
                let gy = ((sy as f32) - v.pan_y) / v.zoom;
                let hit_r = 6.0 / v.zoom + 20.0 / v.zoom;
                let hit = v
                    .tree
                    .query_point(gx as f64, gy as f64, hit_r as f64)
                    .filter(|&i| v.cull.filter_visible[i]);
                (hit, v.anim_mode != AnimMode::Off)
            };

            // A processed click either hits a node or misses (deselects) —
            // record which BEFORE running the side effects below, so a miss
            // that happens to leave the mirror unchanged (already
            // deselected) is still distinguishable in the trail from a click
            // that never reached processing at all (the `click_ignored`
            // sites above).
            match &hit {
                Some(i) => diagnostics::record(diag_instance, "click_hit", Some(format!("node_index={i}"))),
                None => diagnostics::record(diag_instance, "click_miss", None),
            }

            match hit {
                Some(i) => {
                    // A click is the user saying "stop, I want to inspect
                    // this" — the animation is an overview and must not
                    // swallow the interaction. Turning it off drops the
                    // wavefront cull entirely, so the just-selected node (and
                    // its neighbours) render in full context instead of
                    // staying swallowed by e.g. a 3-of-17,814 wavefront cull.
                    // `set_anim_mode` already handles the playing flag, the
                    // toolbar mirror, `recompute_bfs`, and the FULL re-upload
                    // the mode change needs.
                    if was_animating {
                        set_anim_mode(AnimMode::Off);
                    }
                    if let Some(v) = vs.borrow_mut().as_mut() {
                        v.selected = Some(i);
                    }
                    sync_and_upload();
                }
                None => {
                    // A miss deselects. Outbound/Inbound re-seed the BFS
                    // wavefront from the (now empty) selection, which changes
                    // what `cull` admits (see its `Wavefront` doc) — unchanged
                    // from the pre-fix behaviour.
                    let mut cull_changed = false;
                    if let Some(v) = vs.borrow_mut().as_mut() {
                        v.selected = None;
                        if v.anim_mode == AnimMode::Outbound || v.anim_mode == AnimMode::Inbound {
                            v.recompute_bfs();
                            cull_changed = true;
                        }
                    }
                    if cull_changed {
                        full_upload();
                    } else {
                        sync_and_upload();
                    }
                }
            }
        }
    };

    // --- Culling filter: a checkbox flip re-computes the cull sets and
    //     re-uploads (culled items never reach the GPU) ---
    {
        let vs = vs.clone();
        let full_upload = full_upload.clone();
        let alive = alive.clone();
        create_render_effect(move |_| {
            let f = filter.get();
            if !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "filter_effect");
                return;
            }
            if let Some(v) = vs.borrow_mut().as_mut() {
                v.filter = f;
                v.recompute_cull();
            }
            full_upload();
        });
    }

    // --- TRUE CLEAR for the inspector's "‹ back to code graph" (Task 8) ---
    // The inspector can only clear the MIRROR signal
    // (`AppState::selected_code_graph_node`); without this effect the next
    // `sync_and_upload` — e.g. the per-frame animation-playback tick —
    // re-mirrors the still-selected canvas node and the clear never sticks.
    // When the mirror
    // goes to None while the canvas still holds a selection, drop the canvas
    // selection too (this also deselects visually). No ping-pong: a canvas
    // click writes Some (early return here), and `sync_and_upload`'s equality
    // guard won't re-set a mirror that already matches.
    {
        let vs = vs.clone();
        let sync_and_upload = sync_and_upload.clone();
        let alive = alive.clone();
        create_render_effect(move |_| {
            // `state.selected_code_graph_node.get()` MUST run unconditionally
            // (same position as before) to keep this effect subscribed to
            // the mirror signal — only the diagnostic below is new. The
            // `alive` check happens exactly when it did previously (only
            // once the mirror is confirmed unset, matching the original `||`
            // short-circuit), so this identifies specifically the case where
            // `alive` — not the mirror already being clear — caused the bail.
            let mirror_set = state.selected_code_graph_node.get().is_some();
            if !mirror_set && !*alive.borrow() {
                diagnostics::record_alive_bail(diag_instance, "true_clear_effect");
            }
            if mirror_set || !*alive.borrow() {
                return;
            }
            let had_selection = vs
                .borrow_mut()
                .as_mut()
                .and_then(|v| v.selected.take())
                .is_some();
            if had_selection {
                sync_and_upload();
            }
        });
    }

    view! {
        <div class="code-graph-view__surface">
            {move || gl_error.get().map(|e| view! {
                <div class="code-graph-view__empty">
                    <h2>"WebGL2 unavailable"</h2>
                    <p>"The code-graph view needs a WebGL2 context: " {e}</p>
                </div>
            })}
            <div class="code-graph-view__toolbar">
                <button class:is-active=move || tier.get() == Tier::Modules on:click=move |_| tier.set(Tier::Modules)>
                    "Modules"
                </button>
                <button class:is-active=move || tier.get() == Tier::Functions on:click=move |_| tier.set(Tier::Functions)>
                    "Functions"
                </button>
                // "How this map was made" (Item 1): the facts that are NOT
                // per-edge — generator + version, the analysis method in
                // plain words (never the raw fidelity token), and — only
                // when the FIELD says so — that producing this map executed
                // the analysed repo's own code. Lives in the mode chrome
                // (not the inspector) because it describes the whole
                // document, not a selected node/edge; a click-to-toggle
                // popover keeps it discoverable without being a wall of text
                // in the toolbar itself.
                <div class="code-graph-view__provenance">
                    <button
                        class="code-graph-view__provenance-btn"
                        class:is-active=move || provenance_open.get()
                        title="How this map was made"
                        aria-expanded=move || provenance_open.get().to_string()
                        on:click=move |_| provenance_open.update(|o| *o = !*o)
                    >
                        <span aria-hidden="true">"ⓘ "</span>"How this map was made"
                    </button>
                    {move || provenance_open.get().then(|| {
                        let show_exec = discloses_executed_target_code(cg_executed_target_code);
                        view! {
                            <div
                                class="code-graph-view__provenance-popover"
                                role="dialog"
                                aria-label="How this map was made"
                            >
                                <p class="code-graph-view__provenance-line">
                                    {format!("Generator: {cg_generator}")}
                                </p>
                                <p class="code-graph-view__provenance-line">{cg_method_description}</p>
                                {show_exec.then(|| view! {
                                    <p class="code-graph-view__provenance-warn">
                                        "Producing this map executed the analysed repository's \
                                         code (build scripts, proc macros run during analysis)."
                                    </p>
                                })}
                            </div>
                        }
                    })}
                </div>
                <span class="code-graph-view__status">{move || status.get()}</span>
                {move || {
                    let (n, total_n, e, total_e) = counts.get();
                    (n < total_n || e < total_e).then(|| view! {
                        <span class="code-graph-view__notice">
                            {format!("Showing {n} of {total_n} nodes · {e} of {total_e} edges")}
                        </span>
                    })
                }}
                <span class="code-graph-view__fps">{move || fps_label.get()}</span>
            </div>
            <div class="code-graph-view__toolbar code-graph-view__toolbar--filters">
                <span class="code-graph-view__label">"Show:"</span>
                <label class="code-graph-view__check">
                    <input type="checkbox" prop:checked=move || filter.get().show_prod_reachable
                        on:input=move |e| filter.update(|f| f.show_prod_reachable = event_target_checked(&e))/>
                    "Prod-reachable"
                </label>
                <label class="code-graph-view__check">
                    <input type="checkbox" prop:checked=move || filter.get().show_dead
                        on:input=move |e| filter.update(|f| f.show_dead = event_target_checked(&e))/>
                    "Dead (candidates)"
                </label>
                <label class="code-graph-view__check">
                    <input type="checkbox" prop:checked=move || filter.get().show_test_only
                        on:input=move |e| filter.update(|f| f.show_test_only = event_target_checked(&e))/>
                    "Tests"
                </label>
                <label class="code-graph-view__check">
                    <input type="checkbox" prop:checked=move || filter.get().show_generated
                        on:input=move |e| filter.update(|f| f.show_generated = event_target_checked(&e))/>
                    "Generated"
                </label>
                <span class="code-graph-view__label">"Edge kind:"</span>
                <label
                    class="code-graph-view__check"
                    title="Static calls are resolved exactly by either analysis method."
                >
                    <input type="checkbox" prop:checked=move || filter.get().show_static
                        on:input=move |e| filter.update(|f| f.show_static = event_target_checked(&e))/>
                    "Static"
                </label>
                // Fidelity MODULATES this explanation rather than appearing as its
                // own label (Item 1): the same "dynamic" edge kind carries
                // different warranty depending on how the graph was produced —
                // an RTA over-approximation vs. a semantically-resolved target.
                // This toggle is the one place dynamic-vs-static is already
                // communicated to the user, so it is where the explanation lives;
                // there is no per-edge inspect affordance to attach it to instead.
                <label
                    class="code-graph-view__check"
                    title=dynamic_edge_title
                >
                    <input type="checkbox" prop:checked=move || filter.get().show_dynamic
                        on:input=move |e| filter.update(|f| f.show_dynamic = event_target_checked(&e))/>
                    "Dynamic"
                </label>
            </div>
            <div class="code-graph-view__toolbar code-graph-view__toolbar--anim">
                <span class="code-graph-view__label">"Animation:"</span>
                <button class:is-active=move || anim_mode_sig.get() == AnimMode::Off
                    on:click={let f = set_anim_mode.clone(); move |_| f(AnimMode::Off)}>"Off"</button>
                <button class:is-active=move || anim_mode_sig.get() == AnimMode::Roots
                    on:click={let f = set_anim_mode.clone(); move |_| f(AnimMode::Roots)}>"From roots"</button>
                <button class:is-active=move || anim_mode_sig.get() == AnimMode::Outbound
                    on:click={let f = set_anim_mode.clone(); move |_| f(AnimMode::Outbound)}>"Outbound (selected)"</button>
                <button class:is-active=move || anim_mode_sig.get() == AnimMode::Inbound
                    on:click={let f = set_anim_mode.clone(); move |_| f(AnimMode::Inbound)}>"Inbound (selected)"</button>
                <Show when=move || anim_mode_sig.get() != AnimMode::Off>
                    <button on:click={let step = step.clone(); move |_| step(-1)}>"⏮ step"</button>
                    {
                        let play = play.clone();
                        let pause = pause.clone();
                        move || if anim_playing_sig.get() {
                            let pause = pause.clone();
                            view! { <button on:click=move |_| pause()>"⏸ pause"</button> }.into_view()
                        } else {
                            let play = play.clone();
                            view! { <button on:click=move |_| play()>"▶ play"</button> }.into_view()
                        }
                    }
                    <button on:click={let step = step.clone(); move |_| step(1)}>"step ⏭"</button>
                    <input
                        type="range"
                        min="0"
                        max=move || anim_max_sig.get().max(1)
                        prop:value=move || anim_depth_sig.get()
                        on:input={let set_depth = set_depth.clone(); move |e| {
                            if let Ok(d) = event_target_value(&e).parse::<i32>() {
                                set_depth(d);
                            }
                        }}
                    />
                    <span class="code-graph-view__label">
                        {move || format!("depth {} / {}", anim_depth_sig.get(), anim_max_sig.get())}
                    </span>
                    <button class="code-graph-view__reset" title="Stop the animation and reset the wavefront"
                        on:click={let f = set_anim_mode.clone(); move |_| f(AnimMode::Off)}>"⏹ stop & reset"</button>
                </Show>
            </div>
            <div class="code-graph-view__canvas-wrap">
                <canvas
                    node_ref=canvas_ref
                    width="1600"
                    height="1000"
                    class="code-graph-view__canvas"
                    on:mousedown=on_mouse_down
                    on:mousemove=on_mouse_move
                    on:mouseup=end_drag.clone()
                    on:mouseleave=end_drag
                    on:wheel=on_wheel
                    on:click=on_click
                ></canvas>
                {move || render_progress.get().map(|p| {
                    let pct = if p.max_ticks > 0 {
                        (p.ticks as f64 / p.max_ticks as f64 * 100.0).clamp(0.0, 100.0)
                    } else {
                        100.0
                    };
                    let elapsed_s = p.elapsed_ms / 1000.0;
                    let remaining_ticks = p.max_ticks.saturating_sub(p.ticks) as f64;
                    let eta_s = if p.ticks > 0 {
                        (p.elapsed_ms / p.ticks as f64) * remaining_ticks / 1000.0
                    } else {
                        0.0
                    };
                    view! {
                        <div class="code-graph-view__progress" role="status" aria-live="polite">
                            <div class="code-graph-view__progress-stage is-done">
                                <div class="code-graph-view__progress-stage-head">
                                    <span class="code-graph-view__progress-stage-label">
                                        <span class="code-graph-view__progress-stage-check" aria-hidden="true">"✓"</span>
                                        "Building graph model"
                                    </span>
                                    <span class="code-graph-view__progress-stage-detail">
                                        {format!("{} nodes / {} edges — {:.0}ms", p.node_count, p.edge_count, p.build_ms)}
                                    </span>
                                </div>
                                // No bar here on purpose: `build_graph` is one synchronous
                                // call with no sub-progress to show (see `RenderProgress`'s
                                // doc) — a bar that is 100% by construction the instant this
                                // struct exists conveys nothing. The fact worth keeping (it
                                // took ~`build_ms` and cost nothing) is the detail line above;
                                // the checkmark is this stage's completed affordance instead.
                            </div>
                            <div class="code-graph-view__progress-stage is-active">
                                <div class="code-graph-view__progress-stage-head">
                                    <span class="code-graph-view__progress-stage-label">"Laying out graph"</span>
                                    <span class="code-graph-view__progress-stage-detail">
                                        {format!(
                                            "tick {}/{} — {elapsed_s:.1}s elapsed, ~{eta_s:.1}s remaining",
                                            p.ticks, p.max_ticks
                                        )}
                                    </span>
                                </div>
                                <div class="code-graph-view__progress-bar">
                                    <div class="code-graph-view__progress-fill" style=format!("width: {pct:.1}%")></div>
                                </div>
                            </div>
                        </div>
                    }
                })}
            </div>
            <div class="code-graph-view__footer">
                {move || selected_label.get().map(|(label, degree)| view! {
                    <span class="code-graph-view__selected">
                        {format!("Selected: {label} (degree {degree})")}
                    </span>
                })}
            </div>
        </div>
    }
}

#[cfg(test)]
mod refcell_discipline_tests {
    //! Regression coverage for the release-blocking crash captured live via
    //! CDP against the real 17,814-function artifact:
    //! `panicked at .../code_graph_view.rs:853:63: RefCell already borrowed`
    //! → `RuntimeError: unreachable`, trapping the WHOLE wasm instance.
    //!
    //! Root cause: `sync_and_upload`/`full_upload` used to read
    //! `vs.borrow().as_ref()` and keep that borrow alive across
    //! `state.set_selected_code_graph_node(next)` — a Leptos signal write
    //! that runs the "TRUE CLEAR" effect (in `CodeGraphViewCanvas` above)
    //! SYNCHRONOUSLY, and that effect does its own `vs.borrow_mut()`. This
    //! is pure Rust `RefCell` borrow-discipline logic, not GPU or DOM state,
    //! so it is fully reproducible natively — no wasm32 target needed.
    //!
    //! The fixture is a real `GraphModel`/`ViewState` built over the shared
    //! `tests_support::interconnected` generator at 1000+ nodes (the
    //! maintainer's standing convention for call-graph tests — see
    //! `code_graph_view_model::tests`'s "WHY ≥1000" note), so
    //! `snapshot_selection_mirror`'s `v.graph.labels[i]` /
    //! `v.graph.degree[i]` indexing runs against a realistic graph rather
    //! than a toy stub too small to exercise real indices.
    use super::*;
    use crate::code_graph_graph::tests_support;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    /// A 1000-interconnected-function `ViewState` with node 0 selected.
    fn selected_view_state() -> Rc<RefCell<Option<ViewState>>> {
        let (n, edges) = tests_support::interconnected(1000, 3);
        let functions: Vec<serde_json::Value> = (0..n)
            .map(|i| {
                serde_json::json!({
                    "id": format!("f{i}"), "symbol": format!("pkg.fn{i}"), "pkg": "pkg",
                    "file": "f.go", "line": i + 1, "kind": "func", "exported": false,
                    "test": false, "root": i == 0, "generated": false,
                    "reachable": true, "prod_reachable": true,
                    "signature": {"params": [], "results": []},
                    "fan_in": 0, "fan_out": 0
                })
            })
            .collect();
        let calls: Vec<serde_json::Value> = edges
            .iter()
            .map(|&(a, b)| {
                serde_json::json!({
                    "from": format!("f{a}"), "to": format!("f{b}"),
                    "site_file": "f.go", "site_line": 1, "kind": "static"
                })
            })
            .collect();
        let doc: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1", "generator": "magma",
            "language": "go", "module": "example.com/x", "sha": "a",
            "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": functions, "calls": calls
        }))
        .expect("1000-node fixture parses");

        let graph = build_graph(&doc, Tier::Functions);
        let positions = vec![(0.0f32, 0.0f32); n];
        let tree = QuadTree::from_positions_f32(&positions);
        let mut vs = ViewState::new(graph, positions, tree, 0.0, 0.0, 1.0, false);
        vs.selected = Some(0);
        Rc::new(RefCell::new(Some(vs)))
    }

    /// Pins the EXACT pre-fix shape: a `vs.borrow()` still held while a
    /// reentrant `vs.borrow_mut()` runs — standing in for the "TRUE CLEAR"
    /// effect a live `state.set_selected_code_graph_node` write triggers.
    /// This is what `sync_and_upload`/`full_upload` did before the fix, and
    /// reproduces the exact panic message ("RefCell already borrowed")
    /// captured via CDP in production. If this test ever stops panicking,
    /// the fixture below has stopped reproducing the crash it guards.
    #[test]
    fn holding_the_vs_borrow_across_a_reentrant_borrow_mut_panics() {
        let vs = selected_view_state();
        let vs_reentrant = vs.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let guard = vs.borrow(); // the bug: still held below
            let _ = guard.as_ref().map(|v| v.selected);
            let _ = vs_reentrant.borrow_mut(); // the "TRUE CLEAR" effect's touch
        }));
        assert!(
            result.is_err(),
            "holding a `vs.borrow()` across a reentrant `vs.borrow_mut()` must panic \
             (RefCell's documented double-borrow behaviour)"
        );
    }

    /// THE FIX: `snapshot_selection_mirror` — the function `sync_and_upload`/
    /// `full_upload` now call instead of reading `vs` directly — returns
    /// owned data, so its `vs` borrow is released before the caller can
    /// reach a signal write. The SAME reentrant touch that panics above no
    /// longer panics once it happens after the snapshot call returns.
    ///
    /// Before this fix existed, `snapshot_selection_mirror` did not exist —
    /// this test could not even compile, let alone pass. After the fix, it
    /// both compiles and passes, exercising the exact function
    /// `sync_and_upload`/`full_upload` depend on for correctness.
    #[test]
    fn snapshot_selection_mirror_releases_the_borrow_before_returning() {
        let vs = selected_view_state();
        let vs_reentrant = vs.clone();
        let selection_id = |_t: Tier, i: usize| Some(format!("f{i}"));

        let result = catch_unwind(AssertUnwindSafe(|| {
            let snap = snapshot_selection_mirror(&vs, Tier::Functions, &selection_id)
                .expect("a ViewState was set");
            // Stands in for `state.set_selected_code_graph_node` running the
            // "TRUE CLEAR" effect synchronously — must NOT panic now that
            // `snapshot_selection_mirror` has already returned and released
            // its `vs` borrow.
            let _ = vs_reentrant.borrow_mut();
            snap
        }));

        let snap = match result {
            Ok(snap) => snap,
            Err(_) => panic!(
                "snapshot_selection_mirror must release its `vs` borrow before returning \
                 (a reentrant `vs.borrow_mut()` right after the call panicked)"
            ),
        };

        // Not just "didn't panic" — the snapshot must carry the REAL
        // selected node's facts through, at real (1000-node) scale.
        assert_eq!(
            snap.next,
            Some(CodeGraphSelection { tier: Tier::Functions, id: "f0".to_string() }),
            "the mirror must carry the real selected node's id through, not just avoid panicking"
        );
        assert_eq!(
            snap.selected_label.as_ref().map(|(label, _)| label.as_str()),
            Some("pkg.fn0"),
            "the mirror must carry the real selected node's label through"
        );
    }
}
