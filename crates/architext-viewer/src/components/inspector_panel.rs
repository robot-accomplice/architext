//! Fixed-width right inspector.
//!
//! V2: data-bound metadata. In a diagram mode it shows the selected view's (and
//! in flows mode, the selected flow's) metadata in `.accent-surface` cards. In a
//! diagram-less mode it shows a short summary of that mode's data so the panel
//! is never empty. Node-level inspection on diagram click is a V3 concern.
use leptos::*;

use crate::state::use_app_state;
use crate::theme::Mode;

#[component]
pub fn InspectorPanel() -> impl IntoView {
    let state = use_app_state();

    view! {
        <aside class="inspector">
            <div class="overline inspector__section-label">"INSPECTOR"</div>
            {move || {
                let data = state.data.get();
                let mode = state.mode.get();

                // Diagram-less modes summarize their data set.
                if !mode.is_flows() {
                    let (label, count, sample) = match mode {
                        Mode::Rules => ("Rules", data.rules.len(),
                            data.rules.first().map(|r| r.title.clone())),
                        Mode::ReleaseTruth => ("Releases", data.release_index.as_ref()
                            .map(|r| r.releases.len()).unwrap_or(0),
                            data.release_index.as_ref()
                                .and_then(|r| r.current_release_id.clone())),
                        Mode::DataRisks => ("Risks", data.risks.len(),
                            data.risks.first().map(|r| r.title.clone())),
                        _ => ("Nodes", data.nodes.len(),
                            data.nodes.first().map(|n| n.name.clone())),
                    };
                    return view! {
                        <div class="accent-surface">
                            <h2 class="inspector__title">{format!("{label} ({count})")}</h2>
                            <p class="inspector__meta">
                                {sample.map(|s| format!("e.g. {s}"))
                                    .unwrap_or_else(|| "No items".to_string())}
                            </p>
                        </div>
                    }.into_view();
                }

                // Flows mode: selected view + flow metadata.
                let view = state.view_idx.get().and_then(|i| data.views.get(i).cloned());
                let flow = state.flow_idx.get().and_then(|i| data.flows.get(i).cloned());

                view! {
                    {view.map(|v| view! {
                        <div class="accent-surface inspector__card">
                            <div class="overline">"VIEW"</div>
                            <h2 class="inspector__title">{v.name.clone()}</h2>
                            <span class="chip">{v.view_type.clone()}</span>
                            {v.summary.clone().map(|s| view! {
                                <p class="inspector__meta">{s}</p>
                            })}
                        </div>
                    })}
                    {flow.map(|f| view! {
                        <div class="accent-surface inspector__card">
                            <div class="overline">"FLOW"</div>
                            <h2 class="inspector__title">{f.name.clone()}</h2>
                            {f.status.clone().map(|s| view! { <span class="chip">{s}</span> })}
                            {f.summary.clone().map(|s| view! {
                                <p class="inspector__meta">{s}</p>
                            })}
                            {f.trigger.clone().map(|t| view! {
                                <p class="inspector__meta">{format!("Trigger: {t}")}</p>
                            })}
                        </div>
                    })}
                }.into_view()
            }}
        </aside>
    }
}
