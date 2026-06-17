//! Fluid center canvas (DESIGN.md rule 3) — the rendered FLOWS diagram (V3).
//!
//! Responsibilities (kept thin; the diagram itself lives in `crate::diagram`):
//!  - compute the `Plan` reactively from the selected (view, flow) + resolved
//!    diagram config, IN-PROCESS (no worker), via `diagram::plan::compute_plan`;
//!  - own the pan/zoom transform signals and the fit-to-viewport math;
//!  - wire the `- ⤢ +` controls, mouse-wheel zoom, and drag-to-pan;
//!  - host `DiagramSvg` and forward node clicks to `AppState`.
//!
//! Non-flows modes have no diagram projection (V4); they keep the placard.

use leptos::*;
use leptos::ev::{MouseEvent, WheelEvent};

use architext_routing::model::Plan;

use crate::data::models::{Flow, Node, View};
use crate::diagram::plan::{compute_plan, layout_config_from_diagram};
use crate::diagram::DiagramSvg;
use crate::state::use_app_state;

/// The bundle the SVG needs to render one flows-mode diagram.
type DiagramInputs = (Plan, Flow, View, Vec<Node>);

// Zoom bounds + step (centralized, not magic literals at call sites).
const ZOOM_MIN: f64 = 0.1;
const ZOOM_MAX: f64 = 4.0;
const ZOOM_STEP: f64 = 1.2;
const FIT_PADDING: f64 = 32.0; // px of breathing room around the fitted diagram

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
        (state.mode.get().is_flows(), state.view_idx.get(), state.flow_idx.get())
    });
    let diagram_inputs = create_rw_signal::<Option<DiagramInputs>>(None);
    create_effect(move |_| {
        let (is_flows, view_idx, flow_idx) = selection_key.get();
        let data = state.data.get_untracked();
        let bundle = (|| {
            if !is_flows {
                return None;
            }
            let view = view_idx.and_then(|i| data.views.get(i).cloned())?;
            let flow = flow_idx.and_then(|i| data.flows.get(i).cloned())?;
            let plan = compute_plan(&view, &flow, &layout_config());
            Some((plan, flow, view, data.nodes.clone()))
        })();
        diagram_inputs.set(bundle);
    });

    // Fit-to-viewport: scale so the whole canvas fits, then center it. This is
    // an imperative action (rAF / button click), so it reads untracked.
    let fit = move || {
        let Some((plan, _, _, _)) = diagram_inputs.get_untracked() else { return };
        let Some(el) = viewport_ref.get_untracked() else { return };
        let rect = el.get_bounding_client_rect();
        let (vw, vh) = (rect.width(), rect.height());
        if plan.canvas_width <= 0.0 || plan.canvas_height <= 0.0 || vw <= 0.0 || vh <= 0.0 {
            return;
        }
        let scale_x = (vw - FIT_PADDING * 2.0) / plan.canvas_width;
        let scale_y = (vh - FIT_PADDING * 2.0) / plan.canvas_height;
        let scale = scale_x.min(scale_y).clamp(ZOOM_MIN, ZOOM_MAX);
        zoom.set(scale);
        // Center the scaled diagram in the viewport.
        pan_x.set((vw - plan.canvas_width * scale) / 2.0);
        pan_y.set((vh - plan.canvas_height * scale) / 2.0);
    };

    // Re-fit whenever the diagram changes (new view/flow → fresh framing).
    create_effect(move |_| {
        // Track the inputs so a selection change re-runs the fit.
        let _ = diagram_inputs.get();
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

    let on_select = Callback::new(move |node_id: String| state.set_selected_node(node_id));

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
                    Some((plan, flow, view, nodes)) => view! {
                        <DiagramSvg
                            plan=plan
                            flow=flow
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
