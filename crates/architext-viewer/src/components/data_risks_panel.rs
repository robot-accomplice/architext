//! Data / Risks side panel (DISPLAY only).
//!
//! Renders alongside the FLOWS diagram in Data/Risks mode (the diagram itself is
//! the selected flow's risk-overlay / dataflow view, rendered by the shared
//! diagram path — this panel does NOT fork it). Lists `data-classification`
//! entries (name, sensitivity, handling) and `risks` entries (title, severity,
//! summary) as `.accent-surface` cards: the data-class rail encodes sensitivity
//! (`--sens-*`), the risk rail encodes severity (`--sev-*`) — each on its OWN
//! ordinal scale, never a `--c4-*` role hue.

use leptos::*;

use crate::severity::{sensitivity_color_var, severity_color_var};
use crate::state::use_app_state;

#[component]
pub fn DataRisksPanel() -> impl IntoView {
    let state = use_app_state();

    view! {
        <div class="data-risks-panel">
            <div class="overline data-risks-panel__section">"DATA CLASSES"</div>
            {move || {
                let data = state.data.get();
                if data.data_classes.is_empty() {
                    return view! { <p class="inspector__meta">"No data classes."</p> }.into_view();
                }
                data.data_classes.iter().map(|c| {
                    let rail = sensitivity_color_var(c.sensitivity.as_deref());
                    let sensitivity = c.sensitivity.clone().unwrap_or_else(|| "unrated".to_string());
                    view! {
                        <div class="accent-surface data-card" style=format!("--accent:{rail}")>
                            <h3 class="data-card__title">{c.name.clone()}</h3>
                            <span class="chip" style=format!("color:{rail}")>{sensitivity}</span>
                            {c.handling.clone().map(|h| view! {
                                <p class="data-card__summary">{h}</p>
                            })}
                        </div>
                    }
                }).collect_view().into_view()
            }}

            <div class="overline data-risks-panel__section">"RISKS"</div>
            {move || {
                let data = state.data.get();
                if data.risks.is_empty() {
                    return view! { <p class="inspector__meta">"No risks."</p> }.into_view();
                }
                data.risks.iter().map(|r| {
                    let rail = severity_color_var(r.severity.as_deref());
                    let severity = r.severity.clone().unwrap_or_else(|| "unrated".to_string());
                    view! {
                        <div class="accent-surface data-card" style=format!("--accent:{rail}")>
                            <h3 class="data-card__title">{r.title.clone()}</h3>
                            <div class="chip-row">
                                <span class="chip" style=format!("color:{rail}")>{severity}</span>
                                {r.status.clone().map(|s| view! { <span class="chip">{s}</span> })}
                            </div>
                            {r.summary.clone().map(|s| view! {
                                <p class="data-card__summary">{s}</p>
                            })}
                        </div>
                    }
                }).collect_view().into_view()
            }}
        </div>
    }
}
