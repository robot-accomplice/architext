//! Fluid center canvas (DESIGN.md rule 3).
//!
//! The canvas region is FLUID — it fills the grid's `1fr` track and is never
//! pinned to a fixed width/height. Background is the single `--canvas-black`.
//! Zoom / fit controls are present as stubs (no diagram data in V1); they show
//! the explicit-controls requirement and the active mode label for orientation.
use leptos::*;

use crate::theme::Mode;

#[component]
pub fn CanvasPanel(active: ReadSignal<Mode>) -> impl IntoView {
    view! {
        <main class="canvas-panel">
            <div class="canvas-panel__surface"></div>
            <div class="canvas-panel__controls">
                // Zoom / fit control stub — wired to the planner in a later slice.
                <button title="Zoom out">"−"</button>
                <button title="Fit to view">"⤢"</button>
                <button title="Zoom in">"+"</button>
            </div>
            <div class="canvas-panel__hint">
                <div class="overline">"CANVAS"</div>
                <p>{move || format!("{} diagram renders here", active.get().label())}</p>
            </div>
        </main>
    }
}
