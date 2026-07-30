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
use crate::code_graph_layout::LayoutDriver;
use crate::code_graph_view_model::{
    build_graph, fit_camera, should_autoplay, AnimMode, Tier, ViewState, LAYOUT_SEED,
};
use crate::data::models::CodeGraph;
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

#[component]
fn CodeGraphViewCanvas(cg: CodeGraph) -> impl IntoView {
    let state = use_app_state();
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

    let alive: Rc<RefCell<bool>> = Rc::new(RefCell::new(true));
    on_cleanup({
        let alive = alive.clone();
        move || *alive.borrow_mut() = false
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
                return;
            }
            if let Some(v) = vs.borrow().as_ref() {
                selected_label.set(v.selected.map(|i| (v.graph.labels[i].clone(), v.graph.degree[i])));
                anim_depth_sig.set(v.anim_current_depth);
                anim_max_sig.set(v.anim_max_depth);
                // Inspector mirror (Task 6): None when nothing is selected, so
                // a cleared canvas selection clears the inspector detail too.
                // The equality guard matters: this closure also runs per
                // layout-settle frame, and an unconditional `set` would
                // re-render the inspector body every frame with the same value.
                let t = tier.get_untracked();
                let next =
                    v.selected.and_then(|i| selection_id(t, i)).map(|id| CodeGraphSelection { tier: t, id });
                if state.selected_code_graph_node.get_untracked() != next {
                    state.set_selected_code_graph_node(next);
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
                return;
            }
            if let Some(v) = vs.borrow().as_ref() {
                counts.set((
                    v.cull.nodes.len(),
                    v.graph.node_count(),
                    v.cull.edges.len(),
                    v.graph.directed_edges.len(),
                ));
                selected_label.set(v.selected.map(|i| (v.graph.labels[i].clone(), v.graph.degree[i])));
                // Depth-chrome mirror: `full_upload` is now the path
                // `set_anim_mode`/`set_depth`/a hop-boundary crossing take
                // (the cull set changes with the wavefront — see their call
                // sites), so it must mirror the depth label/slider itself
                // rather than leaving that to `sync_and_upload` alone, or
                // "depth N / M" and the scrub slider go stale on every mode
                // change and step/scrub.
                anim_depth_sig.set(v.anim_current_depth);
                anim_max_sig.set(v.anim_max_depth);
                // Inspector mirror (Task 6): runs on the cull/tier paths too,
                // so a filter that culls the selection (or a tier switch,
                // which rebuilds `vs` with `selected: None`) also clears the
                // inspector detail. Same equality guard as `sync_and_upload`:
                // this runs per layout-settle frame.
                let t = tier.get_untracked();
                let next =
                    v.selected.and_then(|i| selection_id(t, i)).map(|id| CodeGraphSelection { tier: t, id });
                if state.selected_code_graph_node.get_untracked() != next {
                    state.set_selected_code_graph_node(next);
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
                let Some(v) = guard.as_ref() else { return };
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
        create_render_effect(move |_| {
            let t = tier.get();
            let Some(canvas) = canvas_ref.get() else { return };
            if !*alive.borrow() {
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
                let (zoom, pan_x, pan_y) = fit_camera(&positions, w, h);
                let tree = QuadTree::from_positions_f32(&positions);
                let mut new_vs =
                    ViewState::new(graph, positions, tree, pan_x, pan_y, zoom, prefers_reduced_motion());
                new_vs.layout_settling = false;
                *vs.borrow_mut() = Some(new_vs);
                *layout.borrow_mut() = None; // nothing to tick — already settled
                leptos::logging::log!(
                    "[code-graph-view] layout cache HIT: nodes={n} edges={edge_count} tier={t:?}"
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
                let (zoom, pan_x, pan_y) = fit_camera(&positions, w, h);
                let settling = !driver.is_done();

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
                return;
            }
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
            if doc.hidden() {
                return; // only act on the hidden -> visible transition
            }
            let now = perf_vis.as_ref().map(|p| p.now()).unwrap_or(0.0);
            if now - last_frame_at_vis.get() > STALL_RESUME_THRESHOLD_MS {
                leptos::logging::log!("[code-graph-view] RAF stalled — resuming on visibilitychange");
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
        let sync_and_upload = sync_and_upload.clone();
        let full_upload = full_upload.clone();
        let set_anim_mode = set_anim_mode.clone();
        move |ev: ev::MouseEvent| {
            if moved.get() {
                return; // the mouseup that ends a drag is not a click
            }
            let Some(canvas) = canvas_ref.get_untracked() else { return };
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
                let Some(v) = guard.as_ref() else { return };
                if v.layout_settling {
                    // The painted positions refresh every frame but the
                    // hit-test tree still covers pre-settle positions — a
                    // hit now would select the WRONG node. Selection opens
                    // when the layout settles (status line says so).
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
            if state.selected_code_graph_node.get().is_some() || !*alive.borrow() {
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
                <label class="code-graph-view__check">
                    <input type="checkbox" prop:checked=move || filter.get().show_static
                        on:input=move |e| filter.update(|f| f.show_static = event_target_checked(&e))/>
                    "Static"
                </label>
                <label class="code-graph-view__check">
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
