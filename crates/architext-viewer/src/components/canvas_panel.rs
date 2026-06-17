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

use std::collections::HashMap;

use leptos::*;
use leptos::ev::{MouseEvent, WheelEvent};

use architext_routing::model::Plan;

use crate::data::models::{Flow, Node, View};
use crate::diagram::plan::{compute_plan, compute_structural_plan, layout_config_from_diagram};
use crate::diagram::DiagramSvg;
use crate::selection::child_c4_view_for_node;
use crate::state::use_app_state;
use crate::theme::Mode;

/// The bundle the SVG needs to render one diagram. `flow` is `Some` in flows
/// mode (drives edge kinds); `edge_labels` is populated in structural (C4 /
/// deployment) mode (the relationship labels). Exactly one of the two carries
/// the edge labels.
type DiagramInputs = (Plan, Option<Flow>, HashMap<String, String>, View, Vec<Node>);

// Zoom bounds + step (centralized, not magic literals at call sites).
const ZOOM_MIN: f64 = 0.1;
const ZOOM_MAX: f64 = 4.0;
const ZOOM_STEP: f64 = 1.2;
const FIT_PADDING: f64 = 24.0; // px of breathing room around the fitted content

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
/// the full padded canvas (which includes margins + disconnected-node columns).
fn content_bounds(plan: &Plan) -> Option<ContentBounds> {
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

    for rect in plan.node_rects.values().chain(plan.label_boxes.values()) {
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

#[component]
pub fn CanvasPanel() -> impl IntoView {
    let state = use_app_state();

    // Pan/zoom transform state, owned here and passed into the SVG.
    let pan_x = create_rw_signal(0.0_f64);
    let pan_y = create_rw_signal(0.0_f64);
    let zoom = create_rw_signal(1.0_f64);
    // Container ref — measured for fit-to-viewport.
    let viewport_ref = create_node_ref::<html::Div>();

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

    // Reactive plan compute. The compute output (`Plan` + cloned data) is not a
    // cheap `PartialEq` type, so we don't memo over it directly. Instead a cheap
    // selector memo over the *identity* (is-flows, view idx, flow idx) gates an
    // effect that recomputes the plan IN-PROCESS and stores the bundle in a
    // signal. This recomputes exactly when the selection changes, not on every
    // unrelated signal.
    let selection_key = create_memo(move |_| {
        (state.mode.get(), state.view_idx.get(), state.flow_idx.get())
    });
    let diagram_inputs = create_rw_signal::<Option<DiagramInputs>>(None);
    create_effect(move |_| {
        let (mode, view_idx, flow_idx) = selection_key.get();
        let data = state.data.get_untracked();
        let bundle = (|| {
            let view = view_idx.and_then(|i| data.views.get(i).cloned())?;
            match mode {
                Mode::Flows => {
                    let flow = flow_idx.and_then(|i| data.flows.get(i).cloned())?;
                    let plan = compute_plan(&view, &flow, &layout_config());
                    Some((plan, Some(flow), HashMap::new(), view, data.nodes.clone()))
                }
                // C4 + Deployment: structural plan (node dependencies → edges),
                // labelled by the relationship rule, no flow.
                Mode::C4 | Mode::Deployment => {
                    let structural =
                        compute_structural_plan(&view, &data.nodes, &layout_config());
                    Some((structural.plan, None, structural.edge_labels, view, data.nodes.clone()))
                }
                // Diagram-less modes keep the placard.
                _ => None,
            }
        })();
        diagram_inputs.set(bundle);
    });

    // Fit-to-viewport: scale so the whole canvas fits, then center it. This is
    // an imperative action (rAF / button click), so it reads untracked.
    let fit = move || {
        let Some((plan, _, _, _, _)) = diagram_inputs.get_untracked() else { return };
        let Some(el) = viewport_ref.get_untracked() else { return };
        let Some(bounds) = content_bounds(&plan) else { return };
        let rect = el.get_bounding_client_rect();
        let (vw, vh) = (rect.width(), rect.height());
        let content_w = bounds.max_x - bounds.min_x;
        let content_h = bounds.max_y - bounds.min_y;
        if content_w <= 0.0 || content_h <= 0.0 || vw <= 0.0 || vh <= 0.0 {
            return;
        }
        // As zoomed-in as possible while the whole content box stays in view.
        let scale_x = (vw - FIT_PADDING * 2.0) / content_w;
        let scale_y = (vh - FIT_PADDING * 2.0) / content_h;
        let scale = scale_x.min(scale_y).clamp(ZOOM_MIN, ZOOM_MAX);
        zoom.set(scale);
        // Center the CONTENT box (not the full canvas) in the viewport.
        pan_x.set((vw - content_w * scale) / 2.0 - bounds.min_x * scale);
        pan_y.set((vh - content_h * scale) / 2.0 - bounds.min_y * scale);
    };

    // Re-fit whenever the diagram changes (new view/flow → fresh framing) OR a
    // sidebar collapses/expands (the center track resized → re-frame for the new
    // viewport width).
    create_effect(move |_| {
        // Track the inputs so a selection change re-runs the fit.
        let _ = diagram_inputs.get();
        let _ = state.nav_collapsed.get();
        let _ = state.inspector_collapsed.get();
        // Defer to the next tick so the SVG (and its viewport) is laid out.
        request_animation_frame(fit);
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

    // Selected view/flow identity for the corner placard (kept for context).
    let placard = move || {
        let data = state.data.get();
        let view = state.view_idx.get().and_then(|i| data.views.get(i).cloned());
        let flow = state.flow_idx.get().and_then(|i| data.flows.get(i).cloned());
        (view, flow)
    };

    view! {
        <main class="canvas-panel">
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
                {move || match diagram_inputs.get() {
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
                            on_select=on_select
                        />
                    }.into_view(),
                    None => view! {
                        <p class="canvas-panel__hint">
                            {move || format!(
                                "{} has no diagram projection — see the inspector for its data.",
                                state.mode.get().label(),
                            )}
                        </p>
                    }.into_view(),
                }}
            </div>
            // Identity placard (bottom-left), bound to the selection.
            {move || {
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
            <div class="canvas-panel__controls">
                <button title="Zoom out" on:click=move |_| zoom_by(1.0 / ZOOM_STEP)>"−"</button>
                <button title="Fit to view" on:click=move |_| fit()>"⤢"</button>
                <button title="Zoom in" on:click=move |_| zoom_by(ZOOM_STEP)>"+"</button>
            </div>
        </main>
    }
}
