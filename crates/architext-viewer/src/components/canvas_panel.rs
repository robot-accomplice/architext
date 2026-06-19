//! Fluid center canvas (DESIGN.md rule 3) — the rendered FLOWS diagram (V3).
//!
//! Responsibilities (kept thin; the diagram itself lives in `crate::diagram`):
//!  - compute the `Plan` reactively from the selected (view, flow) + resolved
//!    diagram config, IN-PROCESS (no worker), via `diagram::plan::compute_plan`;
//!  - own the pan/zoom transform signals and the fit-to-viewport math;
//!  - wire the `- ⤢ +` controls, mouse-wheel zoom, and drag-to-pan;
//!  - host `DiagramSvg` and forward node clicks to `AppState`.
//!
//! Flows, C4, and Deployment modes all project through the SAME `DiagramSvg`:
//! flows feed it a flow + flow-routed plan; C4/Deployment feed it a structural
//! plan (built from node `dependencies`) + per-edge labels and no flow. The
//! remaining (diagram-less) modes keep the placard.

use std::collections::{HashMap, HashSet};

use leptos::*;
use leptos::ev::{MouseEvent, WheelEvent};

use architext_routing::model::Plan;
use architext_routing::plan_diagram::plan_diagram;
use architext_routing::plan_request::build_flow_plan_request;

use crate::components::blast_radius_panel::BlastRadiusPanel;
use crate::components::legend::Legend;
use crate::components::release_truth_panel::ReleaseTruthPanel;
use crate::components::repo_tree::RepoTree;
use crate::components::rules_panel::RulesPanel;
use crate::components::spinner::CanvasSpinner;
use crate::components::steps_panel::StepsPanel;
use crate::data::fetch_farm_plan;
use crate::data::models::{Flow, Node, View};
use crate::diagram::plan::{
    compute_structural_plan, layout_config_from_diagram, plan_hash,
};
use crate::diagram::sequence::{build_sequence_layout, SequenceConfig, SequenceLayout};
use crate::diagram::svg::{flow_node_ids, legend_for};
use crate::diagram::{DiagramSvg, SequenceSvg};
use crate::selection::child_c4_view_for_node;
use crate::state::use_app_state;
use crate::theme::Mode;

/// The bundle the SVG needs to render one diagram. `flow` is `Some` in flows
/// mode (drives edge kinds); `edge_labels` is populated in structural (C4 /
/// deployment) mode (the relationship labels). Exactly one of the two carries
/// the edge labels.
type DiagramInputs = (Plan, Option<Flow>, HashMap<String, String>, View, Vec<Node>);

/// The bundle the sequence SVG needs: the computed layout + the selected view
/// (kept for the placard). Sequence mode is NOT a `plan()` diagram, so it has
/// its own input signal parallel to `DiagramInputs`.
type SequenceInputs = (SequenceLayout, View);

// Zoom bounds + step (centralized, not magic literals at call sites).
const ZOOM_MIN: f64 = 0.1;
const ZOOM_MAX: f64 = 4.0;
const ZOOM_STEP: f64 = 1.2;
// Auto-fit margin: a generous floor with a small viewport-proportional bump so
// content never crowds the stage edges. Manual zoom is unaffected.
const FIT_PADDING_MIN: f64 = 48.0; // px of breathing room around fitted content
const FIT_PADDING_FACTOR: f64 = 0.06; // fraction of the smaller viewport dimension

/// Axis-aligned content bounds (min/max corners). `None` if the plan has no
/// renderable geometry.
struct ContentBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

/// The bounding box of everything that actually renders — node rects unioned
/// with label boxes and route polyline points — so `fit` frames the diagram, not
/// the full padded canvas (which includes margins).
///
/// When `in_flow` is `Some`, only the IN-FLOW node rects are unioned (plus all
/// routes + label boxes, which only exist between in-flow endpoints). This is
/// the fix for the orphan-column misframing: the engine parks out-of-flow nodes
/// in a tall side column, and unioning them stretched the bounds so the actual
/// flow rendered tiny in the corner. `None` (structural C4/Deployment mode →
/// no flow, no orphans) unions every node rect, unchanged.
fn content_bounds(plan: &Plan, in_flow: Option<&HashSet<String>>) -> Option<ContentBounds> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    let mut grow = |x0: f64, y0: f64, x1: f64, y1: f64| {
        min_x = min_x.min(x0);
        min_y = min_y.min(y0);
        max_x = max_x.max(x1);
        max_y = max_y.max(y1);
    };

    for (id, rect) in &plan.node_rects {
        // Skip out-of-flow node cards: they're hidden by default and parked in a
        // disconnected column, so framing them would shrink the actual flow.
        // Decision rects (`decision:<step>`) are never out-of-flow, so the
        // `in_flow` membership test keeps them (they aren't real node ids).
        if let Some(set) = in_flow {
            if !set.contains(id) && !id.starts_with("decision:") {
                continue;
            }
        }
        grow(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    }
    for rect in plan.label_boxes.values() {
        grow(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    }
    for route in plan.routes.values() {
        for p in &route.points {
            grow(p.x, p.y, p.x, p.y);
        }
    }

    if min_x.is_finite() && max_x > min_x && max_y > min_y {
        Some(ContentBounds { min_x, min_y, max_x, max_y })
    } else {
        None
    }
}

/// The pan/zoom transform that frames `bounds` in the viewport. Pure so the
/// (subtle) framing math is unit-testable without a DOM.
struct FitTransform {
    scale: f64,
    pan_x: f64,
    pan_y: f64,
}

/// Compute the `<g>` transform (in viewBox units) that centers `bounds` in a
/// `vw`×`vh` (px) viewport, given the SVG's `vb_w`×`vb_h` viewBox.
///
/// The SVG renders with `preserveAspectRatio="xMidYMid meet"`, so the browser
/// first scales the viewBox into the viewport by `meet = min(vw/vb_w, vh/vb_h)`
/// and centers it. Our transform then operates in viewBox coordinates. Two
/// consequences the math must respect, both learned from real misframings:
///   - SCALE is computed against the viewport re-expressed in viewBox units
///     (`vw/meet`, `vh/meet`) — the full visible area, since content drawn
///     outside the viewBox but inside the SVG element rect is still visible.
///   - CENTERING is on the viewBox center (`vb_w/2`, `vb_h/2`), because `meet`
///     centers the whole viewBox in the viewport; centering on the
///     viewport-in-viewBox frame instead double-applies the letterbox offset.
fn compute_fit(bounds: &ContentBounds, vb_w: f64, vb_h: f64, vw: f64, vh: f64) -> Option<FitTransform> {
    let content_w = bounds.max_x - bounds.min_x;
    let content_h = bounds.max_y - bounds.min_y;
    if content_w <= 0.0 || content_h <= 0.0 || vw <= 0.0 || vh <= 0.0 || vb_w <= 0.0 || vb_h <= 0.0 {
        return None;
    }
    let meet = (vw / vb_w).min(vh / vb_h);
    if meet <= 0.0 {
        return None;
    }
    let view_w = vw / meet;
    let view_h = vh / meet;
    let padding = FIT_PADDING_MIN.max(FIT_PADDING_FACTOR * vw.min(vh)) / meet;
    let scale_x = (view_w - padding * 2.0) / content_w;
    let scale_y = (view_h - padding * 2.0) / content_h;
    // FIT scales to EACH diagram: take the tighter axis so the whole content
    // fits with padding, scaling small diagrams UP to fill (and large ones
    // down) — bounded only by the manual zoom range. (Earlier this capped at
    // natural size, leaving small diagrams marooned in a big viewport; the
    // maintainer's call: "fit by definition scales to each diagram.")
    let scale = scale_x.min(scale_y).clamp(ZOOM_MIN, ZOOM_MAX);
    Some(FitTransform {
        scale,
        pan_x: (vb_w - content_w * scale) / 2.0 - bounds.min_x * scale,
        pan_y: (vb_h - content_h * scale) / 2.0 - bounds.min_y * scale,
    })
}

#[component]
pub fn CanvasPanel() -> impl IntoView {
    let state = use_app_state();

    // Pan/zoom transform state, owned here and passed into the SVG.
    let pan_x = create_rw_signal(0.0_f64);
    let pan_y = create_rw_signal(0.0_f64);
    let zoom = create_rw_signal(1.0_f64);
    // Container ref — measured for fit-to-viewport.
    let viewport_ref = create_node_ref::<html::Div>();
    // Legend overlay open/closed state (default minimized; expandable).
    let legend_collapsed = create_rw_signal(true);
    // Whether to show the out-of-flow ("unrelated") node cards (UX #2). Default
    // OFF: the active flow is the sole focus, orphan cards are hidden. Toggling
    // ON reveals them dimmed and re-fits to frame everything (see the fit path).
    let show_unrelated = create_rw_signal(false);

    // The resolved layout config from /api/config (defaults if absent). The
    // dataset is loaded once and never mutated, so this reads untracked — it is
    // only ever called from imperative contexts (the compute effect / fit).
    let layout_config = move || {
        state
            .data
            .get_untracked()
            .config
            .as_ref()
            .map(|c| layout_config_from_diagram(&c.diagram))
            .unwrap_or_else(|| layout_config_from_diagram(&serde_json::Value::Null))
    };

    // The resolved sequence dims from `/api/config`'s `diagram.sequence`
    // (defaults if absent) — the SEQUENCE analogue of `layout_config`.
    let sequence_config = move || {
        state
            .data
            .get_untracked()
            .config
            .as_ref()
            .map(|c| SequenceConfig::from_diagram(&c.diagram))
            .unwrap_or_default()
    };

    // Reactive plan compute. The compute output (`Plan` + cloned data) is not a
    // cheap `PartialEq` type, so we don't memo over it directly. Instead a cheap
    // selector memo over the *identity* (is-flows, view idx, flow idx) gates an
    // effect that recomputes the plan IN-PROCESS and stores the bundle in a
    // signal. This recomputes exactly when the selection changes, not on every
    // unrelated signal.
    //
    // The key folds in a cheap identity of the resolved diagram config (the
    // `diagram` JSON, stringified). A config write (POST /api/config →
    // `AppState::set_config`) replaces the `data` signal with a new resolved
    // config but leaves mode/view/flow unchanged; tracking the config identity
    // here is what makes the diagram REFLOW on a layout change (e.g. laneWidth /
    // rowGap) rather than re-rendering the stale plan.
    let selection_key = create_memo(move |_| {
        let config_id = state
            .data
            .get()
            .config
            .as_ref()
            .map(|c| c.diagram.to_string())
            .unwrap_or_default();
        (state.mode.get(), state.view_idx.get(), state.flow_idx.get(), config_id)
    });
    let diagram_inputs = create_rw_signal::<Option<DiagramInputs>>(None);
    let sequence_inputs = create_rw_signal::<Option<SequenceInputs>>(None);
    // True while a (re)compute is in flight (fetch + parse, or in-process
    // compute). Drives the on-canvas progress indicator. Set true when the
    // selection changes, false when the render bundle is stored. Because the
    // flows compute is now async (a `/api/plan/{hash}` fetch), the main thread
    // is free and the indicator actually animates.
    let routing = create_rw_signal(false);
    // Monotonic generation: each selection change bumps it; an async result only
    // commits if it is still the latest generation, so a slow farm fetch for a
    // since-abandoned selection can't clobber a newer diagram.
    let generation = create_rw_signal(0_u64);
    create_effect(move |_| {
        let (mode, view_idx, flow_idx, _config_id) = selection_key.get();
        let data = state.data.get_untracked();
        let gen = generation.get_untracked() + 1;
        generation.set(gen);
        // A compute is starting → show the indicator until the bundle is ready.
        routing.set(true);

        // SEQUENCE is a custom (non-plan) layout; compute it on its own signal.
        // Not in the farm → in-process, but still flagged through `routing` so
        // the indicator behaves uniformly across modes.
        if mode == Mode::Sequence {
            let seq = (|| {
                let view = view_idx.and_then(|i| data.views.get(i).cloned())?;
                let flow = flow_idx.and_then(|i| data.flows.get(i).cloned())?;
                let nodes_by_id: HashMap<&str, &Node> =
                    data.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
                let layout = build_sequence_layout(&flow, &nodes_by_id, &sequence_config());
                Some((layout, view))
            })();
            sequence_inputs.set(seq);
            diagram_inputs.set(None);
            routing.set(false);
            return;
        }

        sequence_inputs.set(None);

        // C4 + Deployment: structural plan (node dependencies → edges), labelled
        // by the relationship rule, no flow. NOT in the farm (flows-only) →
        // compute in-process, synchronously, then clear the indicator.
        match mode {
            Mode::C4 | Mode::Deployment => {
                let bundle = view_idx.and_then(|i| data.views.get(i).cloned()).map(|view| {
                    let structural =
                        compute_structural_plan(&view, &data.nodes, &layout_config());
                    (structural.plan, None, structural.edge_labels, view, data.nodes.clone())
                });
                diagram_inputs.set(bundle);
                routing.set(false);
                return;
            }
            // Flows / Data-Risks render the selected flow as a routed plan; fall
            // through to the async fetch-first path below.
            Mode::Flows | Mode::DataRisks => {}
            // Diagram-less modes keep the placard.
            _ => {
                diagram_inputs.set(None);
                routing.set(false);
                return;
            }
        }

        // FLOWS (and Data-Risks): fetch-first from the serve plan farm.
        //
        // Build the flow plan request (gives `.key` + `.plan_diagram_input`),
        // hash the key, and try `GET /api/plan/{hash}`. On a HIT the farm plan
        // (which IS the in-process plan, serialized) renders directly; on a MISS
        // or any error we fall back to in-process `plan_diagram` — never blank.
        let Some(view) = view_idx.and_then(|i| data.views.get(i).cloned()) else {
            diagram_inputs.set(None);
            routing.set(false);
            return;
        };
        let Some(flow) = flow_idx.and_then(|i| data.flows.get(i).cloned()) else {
            diagram_inputs.set(None);
            routing.set(false);
            return;
        };
        let layout = layout_config();
        let nodes = data.nodes.clone();
        let request =
            build_flow_plan_request(&view.to_routing(), &flow.to_routing(), Some(&layout), "orthogonal");
        let hash = plan_hash(&request.key);

        spawn_local(async move {
            // Farm HIT → deserialized plan; MISS / error → in-process fallback.
            let plan = match fetch_farm_plan(&hash).await {
                Some(plan) => plan,
                None => plan_diagram(&request.plan_diagram_input),
            };
            // Only commit if this is still the active selection (guard against a
            // newer selection that started while this fetch was in flight).
            if generation.get_untracked() == gen {
                diagram_inputs.set(Some((plan, Some(flow), HashMap::new(), view, nodes)));
                routing.set(false);
            }
        });
    });

    // Legend rows derived from ONLY what the current diagram renders: present
    // node types (from the plan's resolved cards) + present relationship kinds
    // (from the structural edge labels; empty in flows mode). Recomputes when
    // the diagram inputs change.
    let legend_model = create_memo(move |_| {
        diagram_inputs.with(|inputs| {
            inputs
                .as_ref()
                .map(|(plan, _flow, edge_labels, _view, nodes)| {
                    let nodes_by_id: HashMap<&str, &Node> =
                        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
                    legend_for(plan, edge_labels, &nodes_by_id)
                })
                .unwrap_or_default()
        })
    });

    // Count of out-of-flow ("unrelated") node cards in the current flows
    // diagram: real node rects whose id is not a flow endpoint. `0` whenever
    // there's no flow (structural modes) — used to gate the toggle button so it
    // appears only when there is actually something to reveal.
    let unrelated_count = create_memo(move |_| {
        diagram_inputs.with(|inputs| {
            inputs
                .as_ref()
                .and_then(|(plan, flow, _, _, _)| {
                    let flow = flow.as_ref()?;
                    let in_flow = flow_node_ids(Some(flow));
                    let n = plan
                        .node_rects
                        .keys()
                        .filter(|id| !id.starts_with("decision:") && !in_flow.contains(*id))
                        .count();
                    Some(n)
                })
                .unwrap_or(0)
        })
    });
    // Plain boolean for the toggle's visibility gate (the `view!` tag parser
    // mis-reads an inline `>`/`>=` in a `when=` attribute as the tag close).
    let has_unrelated = create_memo(move |_| unrelated_count.get() > 0);

    // Fit a content box (min/max corners, in viewBox/plan coordinate units)
    // into the measured viewport: as zoomed-in as possible while the whole box
    // stays in view, then centered. Shared by the plan-diagram and sequence
    // paths so framing is identical.
    //
    // `vb_w`/`vb_h` are the SVG's own viewBox dimensions (plan canvas size, or
    // the sequence content box). The SVG renders with
    // `preserveAspectRatio="xMidYMid meet"`, so the browser ALREADY scales the
    // viewBox into the viewport by `meet = min(vw/vb_w, vh/vb_h)` BEFORE our
    // `<g>` transform applies. Our `zoom`/`pan` therefore live in viewBox units,
    // not viewport pixels — fitting in raw px (as this once did) rendered every
    // diagram at `meet`× its intended size and off-centre, since `vb` rarely
    // equals the viewport.
    let fit_bounds = move |bounds: ContentBounds, vb_w: f64, vb_h: f64| {
        let Some(el) = viewport_ref.get_untracked() else { return };
        let rect = el.get_bounding_client_rect();
        if let Some(t) = compute_fit(&bounds, vb_w, vb_h, rect.width(), rect.height()) {
            zoom.set(t.scale);
            pan_x.set(t.pan_x);
            pan_y.set(t.pan_y);
        }
    };

    // Fit-to-viewport: imperative (rAF / button), reads untracked. Sequence
    // fits its full content box `0,0 → width,height`; plan diagrams fit the
    // tighter rendered-geometry bounds.
    let fit = move || {
        if let Some((layout, _)) = sequence_inputs.get_untracked() {
            // The sequence viewBox IS its content box, so vb == bounds.
            fit_bounds(
                ContentBounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: layout.content_width,
                    max_y: layout.content_height,
                },
                layout.content_width,
                layout.content_height,
            );
            return;
        }
        let Some((plan, flow, _, _, _)) = diagram_inputs.get_untracked() else { return };
        // Frame the IN-FLOW geometry only (the fix for UX #1/#2): the engine
        // parks out-of-flow nodes in a tall side column, so framing them left
        // the actual flow tiny in a corner. When the user opts to SHOW the
        // unrelated nodes, frame the full plan so they're visible.
        let in_flow = flow.as_ref().map(|f| flow_node_ids(Some(f)));
        let restrict = if show_unrelated.get_untracked() { None } else { in_flow.as_ref() };
        let Some(bounds) = content_bounds(&plan, restrict) else { return };
        // The plan viewBox is the full canvas; content is a sub-region of it.
        fit_bounds(bounds, plan.canvas_width, plan.canvas_height);
    };

    // Re-fit whenever the diagram changes (new view/flow → fresh framing) OR a
    // sidebar collapses/expands (the center track resized → re-frame for the new
    // viewport width).
    create_effect(move |_| {
        // Track the inputs so a selection change re-runs the fit.
        let _ = diagram_inputs.get();
        let _ = sequence_inputs.get();
        let _ = state.nav_collapsed.get();
        let _ = state.inspector_collapsed.get();
        // The footer steps panel reduces the canvas height when open; re-fit when
        // it toggles so the diagram re-frames for the resized viewport.
        let _ = state.steps_collapsed.get();
        // Toggling "show unrelated nodes" changes what `fit` frames (flow-only
        // vs the full plan), so re-fit when it flips.
        let _ = show_unrelated.get();
        // Defer past layout: the first rAF runs before the steps-panel/collapse
        // reflow settles, so a second rAF measures the CURRENT stage rect and the
        // diagram ends up centered+fitted for the real viewport.
        request_animation_frame(move || request_animation_frame(fit));
    });

    let zoom_by = move |factor: f64| {
        zoom.update(|z| *z = (*z * factor).clamp(ZOOM_MIN, ZOOM_MAX));
    };

    // Mouse-wheel zoom (prevent the page from scrolling).
    let on_wheel = move |ev: WheelEvent| {
        ev.prevent_default();
        let factor = if ev.delta_y() < 0.0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
        zoom_by(factor);
    };

    // Drag-to-pan: track the pointer between mousedown and mouseup.
    let dragging = create_rw_signal(false);
    let last = create_rw_signal((0.0_f64, 0.0_f64));
    let on_mouse_down = move |ev: MouseEvent| {
        dragging.set(true);
        last.set((ev.client_x() as f64, ev.client_y() as f64));
    };
    let on_mouse_move = move |ev: MouseEvent| {
        if !dragging.get() {
            return;
        }
        let (lx, ly) = last.get();
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        pan_x.update(|p| *p += cx - lx);
        pan_y.update(|p| *p += cy - ly);
        last.set((cx, cy));
    };
    let end_drag = move |_: MouseEvent| dragging.set(false);

    // Node click: in C4 mode, a decomposable node (one with a scoped child C4
    // view) drills DOWN to that child view; otherwise (and in every other mode)
    // it selects the node for the inspector — exactly the JS viewer's behavior.
    let on_select = Callback::new(move |node_id: String| {
        if state.mode.get_untracked() == Mode::C4 {
            let data = state.data.get_untracked();
            if let Some(view) = state.view_idx.get_untracked().and_then(|i| data.views.get(i)) {
                if let Some(child_idx) =
                    child_c4_view_for_node(&data.views, &view.view_type, &node_id)
                {
                    state.set_view(child_idx);
                    return;
                }
            }
        }
        state.set_selected_node(node_id);
    });

    // Click a sequence message → select its step (diagram → steps-panel
    // highlight), the inverse of the steps-panel click → diagram highlight.
    let on_select_step = Callback::new(move |step_id: String| {
        state.set_selected_step(step_id);
    });

    // Selected view/flow identity for the corner placard (kept for context).
    let placard = move || {
        let data = state.data.get();
        let view = state.view_idx.get().and_then(|i| data.views.get(i).cloned());
        let flow = state.flow_idx.get().and_then(|i| data.flows.get(i).cloned());
        (view, flow)
    };

    view! {
        <main class="canvas-panel">
            // The stage holds the absolutely-positioned diagram surface; the
            // footer steps panel is a sibling below it (a grid row), so opening it
            // shrinks the stage and the canvas re-fits (see the fit effect).
            <div class="canvas-panel__stage">
            <div class="canvas-panel__surface"></div>
            <div
                class="canvas-panel__viewport"
                node_ref=viewport_ref
                on:wheel=on_wheel
                on:mousedown=on_mouse_down
                on:mousemove=on_mouse_move
                on:mouseup=end_drag
                on:mouseleave=end_drag
            >
                {move || {
                    // SEQUENCE renders its custom layout; all other diagram
                    // modes render the shared plan-based DiagramSvg.
                    if let Some((layout, _)) = sequence_inputs.get() {
                        return view! {
                            <SequenceSvg
                                layout=layout
                                pan_x=pan_x
                                pan_y=pan_y
                                zoom=zoom
                                selected_node=state.selected_node
                                selected_step=state.selected_step
                                on_select=on_select
                                on_select_step=on_select_step
                            />
                        }.into_view();
                    }
                    match diagram_inputs.get() {
                        Some((plan, flow, edge_labels, view, nodes)) => view! {
                            <DiagramSvg
                                plan=plan
                                flow=flow
                                edge_labels=edge_labels
                                view=view
                                nodes=nodes
                                pan_x=pan_x
                                pan_y=pan_y
                                zoom=zoom
                                selected_node=state.selected_node
                                selected_step=state.selected_step
                                show_unrelated=show_unrelated
                                on_select=on_select
                            />
                        }.into_view(),
                        // Non-diagram surfaces render their own component in the
                        // center region; the rest keep the explanatory placard.
                        None => match state.mode.get() {
                            Mode::RepoTree => view! { <RepoTree/> }.into_view(),
                            Mode::Rules => view! { <RulesPanel/> }.into_view(),
                            Mode::BlastRadius => view! { <BlastRadiusPanel/> }.into_view(),
                            Mode::ReleaseTruth => view! { <ReleaseTruthPanel/> }.into_view(),
                            _ => view! {
                                <p class="canvas-panel__hint">
                                    {move || format!(
                                        "{} has no diagram projection — see the inspector for its data.",
                                        state.mode.get().label(),
                                    )}
                                </p>
                            }.into_view(),
                        },
                    }
                }}
            </div>
            // Identity placard (bottom-left) — it LABELS the canvas, so show it
            // only over an actual diagram/sequence surface (same gate as the
            // zoom controls). The diagram-less list modes (Repo Tree, Rules,
            // Blast Radius, Release Truth) render their own full-height panel;
            // there the placard showed a STALE selection identity overlapping
            // the list, so suppress it.
            {move || {
                if !(diagram_inputs.with(Option::is_some)
                    || sequence_inputs.with(Option::is_some))
                {
                    return None;
                }
                let (view, flow) = placard();
                view.map(|v| view! {
                    <div class="canvas-panel__placard">
                        <div class="overline">"CANVAS"</div>
                        <h2 class="canvas-panel__title">{v.name.clone()}</h2>
                        <div class="chip-row">
                            <span class="chip">{v.view_type.clone()}</span>
                            {flow.map(|f| view! {
                                <span class="chip">{format!("{} steps", f.steps.len())}</span>
                            })}
                        </div>
                    </div>
                })
            }}
            // Zoom/fit controls belong only to the pan/zoom diagram surfaces;
            // the scrollable list surfaces (Repo Tree, Rules) have no transform.
            <Show when=move || {
                diagram_inputs.with(Option::is_some) || sequence_inputs.with(Option::is_some)
            }>
                <div class="canvas-panel__controls">
                    <button title="Zoom out" on:click=move |_| zoom_by(1.0 / ZOOM_STEP)>"−"</button>
                    <button title="Fit to view" on:click=move |_| fit()>"⤢"</button>
                    <button title="Zoom in" on:click=move |_| zoom_by(ZOOM_STEP)>"+"</button>
                </div>
            </Show>
            // "Show unrelated nodes" toggle (UX #2) — only over a flows diagram
            // that actually has out-of-flow nodes to reveal.
            <Show when=move || has_unrelated.get()>
                <button
                    class="canvas-panel__unrelated-toggle"
                    class:is-active=move || show_unrelated.get()
                    on:click=move |_| show_unrelated.update(|s| *s = !*s)
                >
                    {move || {
                        let n = unrelated_count.get();
                        if show_unrelated.get() {
                            format!("Hide {n} unrelated")
                        } else {
                            format!("Show {n} unrelated")
                        }
                    }}
                </button>
            </Show>
            // Type/relationship legend (bottom-left, above the placard). Reflects
            // only the types/kinds present in the current diagram; hidden when the
            // current surface has no diagram (empty model → renders nothing).
            <Legend
                model=Signal::derive(move || legend_model.get())
                collapsed=legend_collapsed
                on_toggle=Callback::new(move |_| legend_collapsed.update(|c| *c = !*c))
            />
            // Routing/loading indicator — shown only while a (re)compute is in
            // flight (the async farm fetch / in-process fallback), removed the
            // moment the render bundle is ready so it never obstructs the loaded
            // diagram. Animates because the compute is async (main thread free).
            <Show when=move || routing.get()>
                <CanvasSpinner label="Routing"/>
            </Show>
            </div>
            // Footer step-navigation panel — belongs to the diagram (canvas
            // column), shown only for modes with an ordered flow.
            <StepsPanel/>
        </main>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> ContentBounds {
        ContentBounds { min_x, min_y, max_x, max_y }
    }

    // A minimal plan: one in-flow node near the origin and one orphan node
    // parked far down the canvas (mirroring the engine's out-of-flow column).
    fn plan_with_orphan() -> Plan {
        serde_json::from_value(serde_json::json!({
            "canvasWidth": 1000.0, "canvasHeight": 1000.0,
            "nodeWidth": 136.0, "nodeHeight": 54.0,
            "laneWidth": 200.0, "rowGap": 60.0, "marginX": 24.0, "marginY": 24.0,
            "visibleNodeIds": ["a", "orphan"],
            "laneIndexByNode": [], "rowIndexByNode": [],
            "nodeRects": [
                ["a", {"x": 100.0, "y": 100.0, "width": 136.0, "height": 54.0}],
                ["orphan", {"x": 100.0, "y": 900.0, "width": 136.0, "height": 54.0}]
            ],
            "routes": [], "labelBoxes": []
        }))
        .expect("valid plan json")
    }

    #[test]
    fn content_bounds_excludes_orphans_when_in_flow_restricted() {
        // The fix for UX #1/#2: restricting to the in-flow set ("a") must NOT
        // stretch the bounds down to the parked orphan at y=900.
        let plan = plan_with_orphan();
        let in_flow: HashSet<String> = ["a".to_string()].into_iter().collect();
        let b = content_bounds(&plan, Some(&in_flow)).expect("bounds");
        assert!(b.max_y < 200.0, "orphan must be excluded, max_y={}", b.max_y);
        assert_eq!(b.min_y, 100.0);
        assert_eq!(b.max_y, 154.0);
    }

    #[test]
    fn content_bounds_includes_all_nodes_when_unrestricted() {
        // Structural mode (None) and "show unrelated" both union every node.
        let plan = plan_with_orphan();
        let b = content_bounds(&plan, None).expect("bounds");
        assert_eq!(b.max_y, 954.0, "all nodes unioned, max_y={}", b.max_y);
    }

    // Map the CONTENT centre through the fit transform + the `meet` letterbox to
    // its on-screen pixel position — the thing the viewer actually renders.
    fn screen_center(b: &ContentBounds, vb_w: f64, vb_h: f64, vw: f64, vh: f64) -> (f64, f64) {
        let t = compute_fit(b, vb_w, vb_h, vw, vh).expect("fit");
        let meet = (vw / vb_w).min(vh / vb_h);
        let letterbox_x = (vw - vb_w * meet) / 2.0;
        let letterbox_y = (vh - vb_h * meet) / 2.0;
        let ccx = (b.min_x + b.max_x) / 2.0;
        let ccy = (b.min_y + b.max_y) / 2.0;
        (
            letterbox_x + (t.pan_x + ccx * t.scale) * meet,
            letterbox_y + (t.pan_y + ccy * t.scale) * meet,
        )
    }

    #[test]
    fn centers_content_when_viewbox_matches_viewport() {
        // No letterbox (viewBox == viewport). Content centred → screen centre.
        let b = bounds(100.0, 100.0, 300.0, 260.0);
        let (sx, sy) = screen_center(&b, 600.0, 600.0, 600.0, 600.0);
        assert!((sx - 300.0).abs() < 0.5, "sx={sx}");
        assert!((sy - 300.0).abs() < 0.5, "sy={sy}");
    }

    #[test]
    fn centers_content_despite_letterbox_aspect_mismatch() {
        // The regression case: a wide viewBox (1122x600, aspect 1.87) in a
        // near-square viewport (678x660) is fit by width and letterboxed
        // vertically (~149px). Content must STILL land at the viewport centre —
        // centering on the viewBox centre, not the viewport-in-viewBox frame.
        let b = bounds(180.0, 72.0, 736.0, 464.0);
        let (sx, sy) = screen_center(&b, 1122.0, 600.0, 678.0, 660.0);
        assert!((sx - 339.0).abs() < 1.0, "sx={sx} (want viewport centre 339)");
        assert!((sy - 330.0).abs() < 1.0, "sy={sy} (want viewport centre 330)");
    }

    #[test]
    fn fit_scales_small_diagram_up_to_fill_the_viewport() {
        // FIT scales to EACH diagram: a small diagram in a large viewport is
        // scaled UP so its limiting axis fills the viewport (minus padding) —
        // NOT capped at natural size. Neither axis overflows.
        let b = bounds(0.0, 0.0, 80.0, 60.0);
        let (vb_w, vb_h) = (200.0, 150.0);
        let (vw, vh) = (1000.0, 800.0);
        let t = compute_fit(&b, vb_w, vb_h, vw, vh).expect("fit");
        let meet = (vw / vb_w).min(vh / vb_h);
        let pad = FIT_PADDING_MIN.max(FIT_PADDING_FACTOR * vw.min(vh));
        // On-screen content size: viewBox units → px (scale, then the meet pre-scale).
        let on_w = 80.0 * t.scale * meet;
        let on_h = 60.0 * t.scale * meet;
        assert!(t.scale > 1.0, "should scale UP, scale={}", t.scale);
        let fills_w = (on_w - (vw - 2.0 * pad)).abs() < 1.0;
        let fills_h = (on_h - (vh - 2.0 * pad)).abs() < 1.0;
        assert!(fills_w || fills_h, "limiting axis must fill: on_w={on_w} on_h={on_h}");
        assert!(
            on_w <= vw - 2.0 * pad + 1.0 && on_h <= vh - 2.0 * pad + 1.0,
            "must not overflow: on_w={on_w} on_h={on_h}"
        );
    }

    #[test]
    fn degenerate_inputs_yield_no_transform() {
        assert!(compute_fit(&bounds(0.0, 0.0, 0.0, 0.0), 100.0, 100.0, 100.0, 100.0).is_none());
        assert!(compute_fit(&bounds(0.0, 0.0, 10.0, 10.0), 0.0, 100.0, 100.0, 100.0).is_none());
        assert!(compute_fit(&bounds(0.0, 0.0, 10.0, 10.0), 100.0, 100.0, 0.0, 100.0).is_none());
    }
}
