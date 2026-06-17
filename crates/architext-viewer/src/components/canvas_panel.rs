//! Fluid center canvas (DESIGN.md rule 3).
//!
//! The canvas region is FLUID — it fills the grid's `1fr` track. V2 has no
//! diagram yet (that is V3); instead the surface shows the SELECTED view/flow
//! identity plus a short data summary (e.g. "system-map · N nodes · M steps")
//! so the canvas is visibly bound to the loaded data and the selection logic.
use leptos::*;

use crate::state::use_app_state;

#[component]
pub fn CanvasPanel() -> impl IntoView {
    let state = use_app_state();

    // Selected view identity + node/lane summary chips.
    let view_summary = move || {
        let data = state.data.get();
        state.view_idx.get().and_then(|i| data.views.get(i).cloned())
    };

    // Selected flow identity (flows mode) + step count.
    let flow_summary = move || {
        let data = state.data.get();
        if !state.mode.get().is_flows() {
            return None;
        }
        state.flow_idx.get().and_then(|i| data.flows.get(i).cloned())
    };

    view! {
        <main class="canvas-panel">
            <div class="canvas-panel__surface"></div>
            <div class="canvas-panel__placard">
                    <div class="overline">"CANVAS"</div>
                    {move || match view_summary() {
                        Some(view) => view! {
                            <div class="canvas-panel__identity">
                                <h2 class="canvas-panel__title">{view.name.clone()}</h2>
                                <div class="chip-row">
                                    <span class="chip">{view.view_type.clone()}</span>
                                    <span class="chip">{format!("{} nodes", view.node_count())}</span>
                                    <span class="chip">{format!("{} lanes", view.lanes.len())}</span>
                                </div>
                                {flow_summary().map(|flow| view! {
                                    <div class="canvas-panel__flow">
                                        <div class="overline">"FLOW"</div>
                                        <p class="canvas-panel__flow-name">{flow.name.clone()}</p>
                                        <span class="chip">{format!("{} steps", flow.steps.len())}</span>
                                    </div>
                                })}
                                <p class="canvas-panel__hint">"Diagram renders here in V3"</p>
                            </div>
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
            <div class="canvas-panel__controls">
                // Zoom / fit control stub — wired to the planner in V3.
                <button title="Zoom out">"−"</button>
                <button title="Fit to view">"⤢"</button>
                <button title="Zoom in">"+"</button>
            </div>
        </main>
    }
}
