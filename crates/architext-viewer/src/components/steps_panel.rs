//! Footer step-navigation panel — the diagram's ordered-flow step list.
//!
//! Faithful to the old viewer's `.steps` section (`viewer/src/main.tsx` ~1740):
//! a header line (flow name + summary + a status chip + a collapse toggle) over a
//! list of step cards. Shown ONLY for modes that render an ordered flow (Flows /
//! Data-Risks routed-plan + Sequence) — i.e. [`Mode::projects_flows`] — and only
//! when an active flow is selected.
//!
//! It belongs to the canvas column (it is about the diagram, not full-width), so
//! it lives inside `CanvasPanel`. Clicking a step card selects it
//! (`AppState::set_selected_step`), which the flows diagram keys its `--accent`
//! active edge on (route id == step id). Decision-branch SUPPORT steps are hidden
//! exactly as the JS list does (see [`is_decision_branch_support_step`]).

use leptos::*;

use crate::flow_step_display::{glyph_for_step, step_card_rows};
use crate::state::use_app_state;
use crate::theme::Mode;

#[component]
pub fn StepsPanel() -> impl IntoView {
    let state = use_app_state();
    let collapsed = state.steps_collapsed;
    let toggle = move |_| collapsed.update(|c| *c = !*c);

    // The active flow, only in a flow-projecting mode. `None` → render nothing
    // (the panel is diagram-owned and only meaningful for ordered flows).
    let active_flow = move || {
        let data = state.data.get();
        if !state.mode.get().projects_flows() {
            return None;
        }
        state.flow_idx.get().and_then(|i| data.flows.get(i).cloned())
    };

    // Pre-resolved `node id → display name` for the `from → to` step labels.
    let node_name = move |id: &str| -> String {
        let data = state.data.get_untracked();
        data.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| id.to_string())
    };

    view! {
        <Show when=move || active_flow().is_some() fallback=|| ()>
            {move || {
                let flow = active_flow().expect("guarded by Show");
                let is_collapsed = collapsed.get();
                let section_class = if is_collapsed {
                    "steps-panel steps-panel--collapsed"
                } else {
                    "steps-panel"
                };

                // Build the visible step cards. In Sequence mode the list mirrors
                // the sequence diagram's message rows (every step, 1-based, no
                // decision-branch folding); in Flows/Data-Risks it mirrors the
                // routed plan (support steps hidden, folded display numbers).
                let is_sequence = state.mode.get() == Mode::Sequence;
                let total = flow.steps.len();
                let cards = step_card_rows(&flow.steps, is_sequence)
                    .into_iter()
                    .map(|row| {
                        let step = &flow.steps[row.index];
                        let id = step.id.clone();
                        let display = row.display_number;
                        let glyph = glyph_for_step(step, row.index, total);
                        let from = node_name(&step.from);
                        let to = node_name(&step.to);
                        let action = step.action.clone();
                        let select_id = id.clone();
                        let card_selected = Signal::derive(move || {
                            state.selected_step.get().as_deref() == Some(id.as_str())
                        });
                        let card_class = move || {
                            if card_selected.get() {
                                "step-card is-active"
                            } else {
                                "step-card"
                            }
                        };
                        view! {
                            <button
                                class=card_class
                                on:click=move |_| state.set_selected_step(select_id.clone())
                            >
                                <span class="step-card__glyph">{glyph}</span>
                                <span class="step-card__num mono">{display.to_string()}</span>
                                <span class="step-card__route">
                                    {from}" → "{to}
                                </span>
                                <span class="step-card__action">{action}</span>
                            </button>
                        }
                    })
                    .collect_view();

                view! {
                    <section class=section_class>
                        <div class="steps-panel__head">
                            <div class="steps-panel__title-line">
                                <div class="overline">"FLOW STEPS"</div>
                                <h2 class="steps-panel__title">{flow.name.clone()}</h2>
                                <Show when=move || !collapsed.get() fallback=|| ()>
                                    {flow.summary.clone().map(|s| view! {
                                        <p class="steps-panel__summary">{s}</p>
                                    })}
                                </Show>
                            </div>
                            <div class="steps-panel__actions">
                                {flow.status.clone().map(|s| view! {
                                    <span class="chip">{s}</span>
                                })}
                                <button
                                    class="steps-panel__toggle"
                                    on:click=toggle
                                >
                                    {move || if collapsed.get() { "Show steps" } else { "Hide steps" }}
                                </button>
                            </div>
                        </div>
                        <Show when=move || !collapsed.get() fallback=|| ()>
                            <div class="steps-panel__list">
                                {cards.clone()}
                            </div>
                        </Show>
                    </section>
                }
            }}
        </Show>
    }
}
