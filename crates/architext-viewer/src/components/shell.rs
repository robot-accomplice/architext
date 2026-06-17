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

#[component]
pub fn Shell() -> impl IntoView {
    view! {
        <div class="shell">
            <LeftNav/>
            <CanvasPanel/>
            <InspectorPanel/>
        </div>
    }
}
