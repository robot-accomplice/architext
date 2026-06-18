//! View / flow selector region (left nav).
//!
//! Driven entirely by the loaded data + the ported routing selection rules:
//! - flows mode shows a FLOW selector (all flows) plus a VIEW selector
//!   restricted to projections compatible with the selected flow;
//! - non-flow modes show only the VIEW selector, restricted to the views that
//!   mode projects.
//!
//! Diagram-less modes (rules, repo-tree, blast-radius) project no views, so the
//! region shows a short explanatory note instead. Release Truth is the one
//! diagram-less mode with its own selector here: a RELEASE picker (over the
//! release index) bound to the shared `state.selected_release`, so choosing a
//! release in the nav drives the center detail.
use leptos::*;

use crate::selection;
use crate::state::use_app_state;
use crate::theme::Mode;

#[component]
pub fn SelectorBar() -> impl IntoView {
    let state = use_app_state();

    // Flow selector (flows mode only).
    let flow_options = move || {
        let data = state.data.get();
        if !state.mode.get().projects_flows() {
            return Vec::new();
        }
        data.flows
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.name.clone()))
            .collect::<Vec<_>>()
    };

    // View selector options: in flows mode, compatible projections for the
    // selected flow; otherwise the mode's view types.
    let view_options = move || {
        let data = state.data.get();
        let mode = state.mode.get();
        let indices = if mode.is_flows() {
            // Flows: every flow-projection view compatible with the selected flow.
            match state.flow_idx.get() {
                Some(f) => selection::compatible_flow_views(&data.views, &data.flows, f),
                None => Vec::new(),
            }
        } else {
            // Every other mode (incl. Data/Risks): the mode's own view types.
            // For Data/Risks that scopes the selector to risk-overlay / dataflow
            // even though the diagram itself renders the selected flow.
            selection::views_for_mode(&data.views, mode)
        };
        indices
            .into_iter()
            .filter_map(|i| data.views.get(i).map(|v| (i, v.name.clone())))
            .collect::<Vec<_>>()
    };

    // Release selector options (Release Truth mode only): the recorded releases,
    // newest first (the index lists oldest→newest), as (id, label) pairs.
    let release_options = move || {
        let data = state.data.get();
        if state.mode.get() != Mode::ReleaseTruth {
            return Vec::new();
        }
        data.release_index
            .as_ref()
            .map(|idx| {
                idx.releases
                    .iter()
                    .rev()
                    .map(|r| {
                        let label = r
                            .version
                            .clone()
                            .or_else(|| r.name.clone())
                            .unwrap_or_else(|| r.id.clone());
                        (r.id.clone(), label)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    view! {
        <div class="selector-bar">
            <Show when=move || state.mode.get().projects_flows()>
                <div class="selector-bar__group">
                    <div class="overline">"FLOW"</div>
                    <select
                        class="selector-bar__select"
                        on:change=move |ev| {
                            if let Ok(idx) = event_target_value(&ev).parse::<usize>() {
                                state.set_flow(idx);
                            }
                        }
                    >
                        {move || flow_options()
                            .into_iter()
                            .map(|(i, name)| {
                                let selected = state.flow_idx.get() == Some(i);
                                view! {
                                    <option value=i.to_string() selected=selected>{name}</option>
                                }
                            })
                            .collect_view()}
                    </select>
                </div>
            </Show>

            // Release Truth: a RELEASE selector (not a VIEW one) bound to the
            // shared selection signal, so picking here drives the center detail.
            <Show when=move || state.mode.get() == Mode::ReleaseTruth>
                <div class="selector-bar__group">
                    <div class="overline">"RELEASE"</div>
                    <select
                        class="selector-bar__select"
                        on:change=move |ev| {
                            let id = event_target_value(&ev);
                            state.selected_release.set((!id.is_empty()).then_some(id));
                        }
                    >
                        {move || release_options()
                            .into_iter()
                            .map(|(id, label)| {
                                let selected = state.selected_release.get().as_deref() == Some(id.as_str());
                                view! {
                                    <option value=id.clone() selected=selected>{label}</option>
                                }
                            })
                            .collect_view()}
                    </select>
                </div>
            </Show>

            // VIEW selector for diagram modes.
            <Show when=move || !view_options().is_empty()>
                <div class="selector-bar__group">
                    <div class="overline">"VIEW"</div>
                    <select
                        class="selector-bar__select"
                        on:change=move |ev| {
                            if let Ok(idx) = event_target_value(&ev).parse::<usize>() {
                                state.set_view(idx);
                            }
                        }
                    >
                        {move || view_options()
                            .into_iter()
                            .map(|(i, name)| {
                                let selected = state.view_idx.get() == Some(i);
                                view! {
                                    <option value=i.to_string() selected=selected>{name}</option>
                                }
                            })
                            .collect_view()}
                    </select>
                </div>
            </Show>

            // The "no diagram projection" note: only for diagram-less modes that
            // have NO selector here (Repo Tree, Rules, Blast Radius). Release
            // Truth is excluded — it shows the RELEASE selector above.
            <Show when=move || {
                view_options().is_empty() && state.mode.get() != Mode::ReleaseTruth
            }>
                <p class="selector-bar__note">
                    {move || format!("{} has no diagram projection", state.mode.get().label())}
                </p>
            </Show>
        </div>
    }
}
