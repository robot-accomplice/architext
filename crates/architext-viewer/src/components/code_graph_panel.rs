//! The Code Graph mode: renders Magma's `code-graph.json` as a layered
//! node-link diagram.
//!
//! Mirrors the `BlastRadiusPanel` shape — a mode component that renders its own
//! centre-canvas surface rather than a routed diagram. Selection is PANEL-LOCAL:
//! code-graph ids live in their own id-space and must never be written into
//! `AppState::selected_node`, which the inspector resolves against `data.nodes`.
//!
//! Four surfaces, in precedence order:
//!   1. no document registered  → how-to-generate empty state
//!   2. document unreadable     → explicit error surface (never a blank canvas)
//!   3. refusal (`computable:false`) → the producer's reason, verbatim
//!   4. a graph                 → the diagram (Task 4 onward)
use leptos::*;

use crate::state::use_app_state;

#[component]
pub fn CodeGraphPanel() -> impl IntoView {
    let state = use_app_state();

    view! {
        <div class="code-graph">
            {move || {
                let data = state.data.get();
                match &data.code_graph {
                    // 1. Not registered in the manifest.
                    None => view! {
                        <div class="code-graph__empty">
                            <h2 class="code-graph__empty-title">"No code graph yet"</h2>
                            <p class="code-graph__empty-body">
                                "This project has no " <code>"code-graph.json"</code> ". \
                                 Generate one with Magma, then re-run " <code>"architext doctor"</code>
                                " to register it:"
                            </p>
                            <pre class="code-graph__empty-cmd">"magma --architext <repo> <name> <vault>"</pre>
                            <p class="code-graph__empty-note">
                                "Go modules only — other languages report why the graph could not be built."
                            </p>
                        </div>
                    }.into_view(),
                    // 2. Registered but unreadable/malformed — say so loudly.
                    Some(Err(err)) => view! {
                        <div class="code-graph__empty">
                            <h2 class="code-graph__empty-title">"Code graph could not be read"</h2>
                            <p class="code-graph__empty-body">{err.to_string()}</p>
                            <p class="code-graph__empty-note">
                                "The rest of the architecture loaded normally. Re-run the producer, \
                                 then " <code>"architext validate"</code> " to see the specific defect."
                            </p>
                        </div>
                    }.into_view(),
                    // 3. A refusal is a VALID document — surface the reason verbatim
                    //    and synthesize nothing.
                    Some(Ok(cg)) if !cg.computable => {
                        let reason = cg.not_computable_reason.clone()
                            .unwrap_or_else(|| "no reason given".to_string());
                        let lang = cg.language.clone();
                        view! {
                            <div class="code-graph__empty">
                                <h2 class="code-graph__empty-title">"No graph available"</h2>
                                <p class="code-graph__empty-body">{reason}</p>
                                <p class="code-graph__empty-note">
                                    {format!("The producer analysed this project as \"{lang}\" and \
                                              declined to guess. Nothing is inferred.")}
                                </p>
                            </div>
                        }.into_view()
                    }
                    // 4. A real graph — the diagram lands here in Task 4.
                    Some(Ok(_)) => view! {
                        <div class="code-graph__pending">"Graph rendering arrives in Task 4."</div>
                    }.into_view(),
                }
            }}
        </div>
    }
}
