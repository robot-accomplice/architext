//! Fixed-width right inspector.
//!
//! V1 placeholder: an `.overline` section label + an `.accent-surface` example
//! card (the 3px --accent rail primitive). No selection data yet.
use leptos::*;

#[component]
pub fn InspectorPanel() -> impl IntoView {
    view! {
        <aside class="inspector">
            <div class="overline inspector__section-label">"INSPECTOR"</div>
            <div class="accent-surface">
                <h2 class="inspector__title">"Selection details"</h2>
                <p class="inspector__meta">"Select a node to inspect it"</p>
            </div>
        </aside>
    }
}
