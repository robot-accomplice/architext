//! App shell: the fixed-fluid console grid (DESIGN.md "Layout").
//!
//! fixed ~280px left nav | FLUID center canvas | fixed ~320px right inspector,
//! joined by 1px hairline gaps. State (mode/view/flow + loaded data) lives in
//! `AppState`, provided via context by `App` before this renders; the panels
//! read it from context.
use leptos::*;

use crate::components::canvas_panel::CanvasPanel;
use crate::components::inspector_panel::InspectorPanel;
use crate::components::left_nav::LeftNav;
use crate::state::use_app_state;

#[component]
pub fn Shell() -> impl IntoView {
    let state = use_app_state();

    // The fixed tracks collapse to a thin rail; the center canvas grows to fill
    // the freed space. Driving grid-template-columns reactively (not a class)
    // keeps the column widths single-sourced from the collapse signals.
    let grid_columns = move || {
        let nav = if state.nav_collapsed.get() {
            "var(--nav-collapsed)"
        } else {
            "var(--nav-w)"
        };
        let inspector = if state.inspector_collapsed.get() {
            "var(--inspector-collapsed)"
        } else {
            "var(--inspector-w)"
        };
        format!("grid-template-columns: {nav} 1fr {inspector};")
    };

    view! {
        <div class="shell" style=grid_columns>
            <LeftNav/>
            <CanvasPanel/>
            <InspectorPanel/>
        </div>
    }
}
