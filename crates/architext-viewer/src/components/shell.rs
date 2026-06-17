//! App shell: the fixed-fluid console grid (DESIGN.md "Layout").
//!
//! fixed ~280px left nav | FLUID center canvas | fixed ~320px right inspector,
//! joined by 1px hairline gaps. Holds the active-mode signal (local state for
//! V1; data routing arrives in a later slice).
use leptos::*;

use crate::components::canvas_panel::CanvasPanel;
use crate::components::inspector_panel::InspectorPanel;
use crate::components::left_nav::LeftNav;
use crate::theme::Mode;

#[component]
pub fn Shell() -> impl IntoView {
    let (active, set_active) = create_signal(Mode::Flows);

    view! {
        <div class="shell">
            <LeftNav active=active set_active=set_active/>
            <CanvasPanel active=active/>
            <InspectorPanel/>
        </div>
    }
}
