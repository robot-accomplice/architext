//! View / flow selector region (left nav).
//!
//! Driven entirely by the loaded data + the ported routing selection rules:
//! - flows mode shows a FLOW selector (all flows) plus a VIEW selector
//!   restricted to projections compatible with the selected flow;
//! - non-flow modes show only the VIEW selector, restricted to the views that
//!   mode projects.
//!
//! Diagram-less modes (rules, release-truth, repo-tree, blast-radius) project
//! no views, so the region shows a short explanatory note instead.
use leptos::*;

use crate::selection;
use crate::state::use_app_state;

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
            match state.flow_idx.get() {
                Some(f) => selection::compatible_flow_views(&data.views, &data.flows, f),
                None => Vec::new(),
            }
        } else {
            selection::views_for_mode(&data.views, mode)
        };
        indices
            .into_iter()
            .filter_map(|i| data.views.get(i).map(|v| (i, v.name.clone())))
            .collect::<Vec<_>>()
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

            <Show
                when=move || !view_options().is_empty()
                fallback=move || view! {
                    <p class="selector-bar__note">
                        {move || format!("{} has no diagram projection", state.mode.get().label())}
                    </p>
                }
            >
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
        </div>
    }
}
