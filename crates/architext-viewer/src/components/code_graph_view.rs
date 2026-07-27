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
use crate::code_graph_view_model::{build_graph, fit_zoom, AnimMode, Tier, ViewState};
use crate::data::models::CodeGraph;
use crate::force_layout::ForceConfig;
use crate::gl::renderer::Renderer;
use crate::state::{use_app_state, CodeGraphSelection};

/// Per-frame millisecond budget for progressive layout ticks (Task 5). At
/// 17,561 nodes one tick costs ~15-20 ms, so the budget usually buys one
/// tick per frame there — the frame rate dips during the settle but every
/// frame still yields to input/paint, and the `step_within` contract
/// guarantees at least one tick so a slow tick can't starve progress.
/// Small tiers blow through their whole layout in the first frame or two.
const LAYOUT_FRAME_BUDGET_MS: f64 = 12.0;

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
    let anim_mode_sig = create_rw_signal(AnimMode::Off);
    let anim_depth_sig = create_rw_signal(0i32);
    let anim_max_sig = create_rw_signal(0i32);
    let anim_playing_sig = create_rw_signal(false);

    // GPU + view state live OUTSIDE Leptos signals (see ViewState docs).
    let gpu: Rc<RefCell<Option<Renderer>>> = Rc::new(RefCell::new(None));
    let vs: Rc<RefCell<Option<ViewState>>> = Rc::new(RefCell::new(None));
    let interval: Rc<RefCell<Option<gloo_timers::callback::Interval>>> = Rc::new(RefCell::new(None));
    // Progressive layout (Task 5): the seeded, still-ticking driver for the
    // current tier. The RAF loop slices it; `None` once settled. Plain
    // `Cell`s for the instrumentation/camera bookkeeping — nothing reads
    // them reactively (same rationale as the drag `Cell`s below).
    let layout: Rc<RefCell<Option<LayoutDriver>>> = Rc::new(RefCell::new(None));
    let layout_t0: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let first_paint_logged: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let user_moved_camera: Rc<Cell<bool>> = Rc::new(Cell::new(false));

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
    let set_anim_mode = {
        let vs = vs.clone();
        let sync_and_upload = sync_and_upload.clone();
        let interval = interval.clone();
        let alive = alive.clone();
        move |mode: AnimMode| {
            *interval.borrow_mut() = None;
            if !*alive.borrow() {
                return;
            }
            anim_playing_sig.set(false);
            anim_mode_sig.set(mode);
            if let Some(v) = vs.borrow_mut().as_mut() {
                v.anim_mode = mode;
                v.recompute_bfs();
            }
            sync_and_upload();
        }
    };
    let set_depth = {
        let vs = vs.clone();
        let sync_and_upload = sync_and_upload.clone();
        let alive = alive.clone();
        move |d: i32| {
            if let Some(v) = vs.borrow_mut().as_mut() {
                v.anim_current_depth = d.clamp(0, v.anim_max_depth);
            }
            if !*alive.borrow() {
                return;
            }
            sync_and_upload();
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
        let interval = interval.clone();
        let sync_and_upload = sync_and_upload.clone();
        let alive = alive.clone();
        move || {
            if !*alive.borrow() {
                return;
            }
            // Pressing play after the wavefront reached the end RESTARTS it
            // from depth 0 rather than freezing on the final frame.
            if let Some(v) = vs.borrow_mut().as_mut() {
                if v.anim_current_depth >= v.anim_max_depth {
                    v.anim_current_depth = 0;
                }
            }
            anim_playing_sig.set(true);
            let vs2 = vs.clone();
            let sync2 = sync_and_upload.clone();
            let interval2 = interval.clone();
            let alive2 = alive.clone();
            let tick = gloo_timers::callback::Interval::new(400, move || {
                if !*alive2.borrow() {
                    *interval2.borrow_mut() = None;
                    return;
                }
                let mut done = false;
                if let Some(v) = vs2.borrow_mut().as_mut() {
                    if v.anim_current_depth >= v.anim_max_depth {
                        done = true;
                    } else {
                        v.anim_current_depth += 1;
                    }
                }
                sync2();
                if done {
                    *interval2.borrow_mut() = None;
                    anim_playing_sig.set(false);
                }
            });
            *interval.borrow_mut() = Some(tick);
        }
    };
    let pause = {
        let interval = interval.clone();
        let alive = alive.clone();
        move || {
            *interval.borrow_mut() = None;
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
        let interval = interval.clone();
        let full_upload = full_upload.clone();
        let layout = layout.clone();
        let layout_t0 = layout_t0.clone();
        let first_paint_logged = first_paint_logged.clone();
        let user_moved_camera = user_moved_camera.clone();
        let set_anim_mode = set_anim_mode.clone();
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

            *interval.borrow_mut() = None; // stop the previous tier's animation

            let graph = build_graph(&cg, t);
            let n = graph.node_count();
            let edge_count = graph.directed_edges.len();
            let seed = 1_469_598_103_934_665_603u64; // fixed — reproducible layout
            let driver = LayoutDriver::new(n, &graph.layout_edges, seed, &ForceConfig::default());
            // Tick-0 positions (the seeded circle) upload IMMEDIATELY so the
            // first frame paints a real graph, not a spinner.
            let positions = driver.positions_f32();
            let (w, h) = (canvas.width() as f32, canvas.height() as f32);
            let zoom = fit_zoom(&positions, w, h);
            let settling = !driver.is_done();

            // `vs` MUST be replaced BEFORE any of the signal writes below:
            // each `.set()` synchronously re-runs the filter effect (which
            // re-uploads, sized from `vs`). Writing the signals first let
            // that reentrant upload fire while `vs` still held the PREVIOUS
            // tier's counts against buffers just reallocated to the NEW
            // tier's size — a `bufferSubData` overflow (the spike's bug,
            // kept fixed). The initial hit-test tree covers the seeded
            // positions; the RAF loop replaces it with the settled tree.
            let mut new_vs = ViewState::new(graph, positions, driver.hit_tree(), w / 2.0, h / 2.0, zoom);
            new_vs.layout_settling = settling;
            *vs.borrow_mut() = Some(new_vs);
            *layout.borrow_mut() = Some(driver);
            user_moved_camera.set(false);
            first_paint_logged.set(false);
            layout_t0.set(web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0));
            leptos::logging::log!("[code-graph-view] layout seeded: nodes={n} edges={edge_count} settling={settling}");
            full_upload();

            if settling {
                status.set(format!("{n} nodes / {edge_count} edges — layout settling…"));
            } else {
                status.set(format!("{n} nodes / {edge_count} edges"));
            }
            filter.set(FilterState::default());
            // AUTO-PLAY ON OPEN is deferred to the settle (RAF loop below):
            // starting the wavefront while positions churn would sweep the
            // animation across a graph that is still moving.
            set_anim_mode(AnimMode::Off);
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
        let full_upload = full_upload.clone();
        let set_anim_mode = set_anim_mode.clone();
        let play = play.clone();
        let layout_t0 = layout_t0.clone();
        let first_paint_logged = first_paint_logged.clone();
        let user_moved_camera = user_moved_camera.clone();
        let perf = web_sys::window().and_then(|w| w.performance());
        type FrameCb = wasm_bindgen::closure::Closure<dyn FnMut(f64)>;
        let frame_cb: Rc<RefCell<Option<FrameCb>>> = Rc::new(RefCell::new(None));
        let frame_times: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::with_capacity(120)));
        let frame_cb2 = frame_cb.clone();
        *frame_cb.borrow_mut() = Some(wasm_bindgen::closure::Closure::wrap(Box::new(move |now: f64| {
            if !*alive.borrow() {
                return;
            }
            // --- Progressive layout slice (Task 5) ---
            // Spend up to LAYOUT_FRAME_BUDGET_MS ticking the seeded layout,
            // then hand the frame back to input/paint. Positions are
            // re-uploaded every ticked frame so the user WATCHES the graph
            // settle; the status line mirrors tick progress.
            let mut ticked = false;
            let mut settled_ticks: Option<usize> = None;
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
                        let (ticks, max) = (d.ticks_run(), d.max_ticks());
                        if let Some(v) = vs.borrow_mut().as_mut() {
                            v.positions = positions;
                            if *alive.borrow() {
                                status.set(format!(
                                    "{} nodes / {} edges — settling layout (tick {ticks}/{max})",
                                    v.graph.node_count(),
                                    v.graph.directed_edges.len()
                                ));
                            }
                        }
                        if done {
                            settled_ticks = Some(ticks);
                            let tree = d.hit_tree();
                            // Re-fit to the settled extent — but never stomp a
                            // camera the user already moved during the settle.
                            let refit = !user_moved_camera.get();
                            if let Some(v) = vs.borrow_mut().as_mut() {
                                v.tree = tree;
                                v.layout_settling = false;
                                if refit {
                                    if let Some(canvas) = canvas_ref.get_untracked() {
                                        v.zoom = fit_zoom(
                                            &v.positions,
                                            canvas.width() as f32,
                                            canvas.height() as f32,
                                        );
                                    }
                                }
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
            if let Some(ticks) = settled_ticks {
                if *alive.borrow() {
                    let (n, e) = vs
                        .borrow()
                        .as_ref()
                        .map(|v| (v.graph.node_count(), v.graph.directed_edges.len()))
                        .unwrap_or((0, 0));
                    status.set(format!("{n} nodes / {e} edges — {ticks} ticks"));
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
                    if tier.get_untracked() == Tier::Functions {
                        set_anim_mode(AnimMode::Roots);
                        play();
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
            if let Some(v) = vs.borrow_mut().as_mut() {
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
                // The tree spans the FULL graph (layout stability); a CULLED
                // node is not uploaded and must not be selectable through it.
                v.selected =
                    v.tree.query_point(gx as f64, gy as f64, hit_r as f64).filter(|&i| v.cull.visible[i]);
                if v.anim_mode == AnimMode::Outbound || v.anim_mode == AnimMode::Inbound {
                    v.recompute_bfs();
                }
            }
            sync_and_upload();
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
