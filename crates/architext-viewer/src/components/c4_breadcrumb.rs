//! C4 drill-down breadcrumb trail (canvas overlay, C4 mode only).
//!
//! C4 views form a hierarchy — Context → Container → Component → Code — anchored
//! by `scopeNodeId`: a view W is the drill-down target of node N when
//! `W.scopeNodeId == N.id`. Clicking a node with a scoped child view drills DOWN
//! (see `canvas_panel`'s `on_select`), pushing the child onto `state.c4_trail`.
//! This component renders that trail as a clickable path so the user can always
//! climb back UP: `Context › Container: Architext CLI › Component: …`. Clicking
//! a crumb navigates to that level (truncating the descendants); the last crumb
//! is the active view and is rendered as non-interactive "current".
//!
//! Pinned top-left of the canvas, on-language: an overline eyebrow, the path in
//! mono with `›` separators, the active crumb in `--accent`. Renders nothing
//! unless the trail has at least two crumbs (a single root needs no breadcrumb —
//! the canvas placard already names the view).

use leptos::*;

use crate::state::use_app_state;
use crate::theme::Mode;

/// Short, human C4 level name for a `c4-*` view type. The crumb pairs this with
/// the scoped node's name so the path reads as level + what was drilled into.
fn c4_level_label(view_type: &str) -> &'static str {
    match view_type {
        "c4-context" => "Context",
        "c4-container" => "Container",
        "c4-component" => "Component",
        "c4-code" => "Code",
        _ => "View",
    }
}

/// One resolved crumb: the trail depth, its display label, and whether it is the
/// active (last) crumb.
#[derive(Clone)]
struct Crumb {
    depth: usize,
    label: String,
    is_current: bool,
}

#[component]
pub fn C4Breadcrumb() -> impl IntoView {
    let state = use_app_state();

    // Resolve the trail of view indices into display crumbs. The root crumb is
    // just its C4 level ("Context"); a drilled crumb appends the scoped node's
    // name ("Container: Architext CLI") so the path says what was opened. Falls
    // back to the view name / level alone if the scope node can't be resolved.
    let crumbs = move || -> Vec<Crumb> {
        if state.mode.get() != Mode::C4 {
            return Vec::new();
        }
        let data = state.data.get();
        let trail = state.c4_trail.get();
        let last = trail.len().saturating_sub(1);
        trail
            .iter()
            .enumerate()
            .filter_map(|(depth, &view_idx)| {
                let view = data.views.get(view_idx)?;
                let level = c4_level_label(&view.view_type);
                // A scoped (drilled-into) view names the node it decomposes; the
                // crumb reads "Level: NodeName". The root view has no scope.
                let label = match view.scope_node_id.as_deref() {
                    Some(node_id) => {
                        let node_name = data
                            .nodes
                            .iter()
                            .find(|n| n.id == node_id)
                            .map(|n| n.name.clone())
                            .unwrap_or_else(|| node_id.to_string());
                        format!("{level}: {node_name}")
                    }
                    None => level.to_string(),
                };
                Some(Crumb { depth, label, is_current: depth == last })
            })
            .collect()
    };

    view! {
        // Only render once there is somewhere to navigate back to (≥2 crumbs);
        // a lone root is already labeled by the canvas placard.
        <Show when=move || { crumbs().len() > 1 }>
            <nav class="c4-breadcrumb" aria-label="C4 drill-down path">
                <span class="overline">"DRILL PATH"</span>
                <ol class="c4-breadcrumb__trail">
                    {move || crumbs()
                        .into_iter()
                        .map(|c| {
                            let Crumb { depth, label, is_current } = c;
                            // The active crumb is plain text (you're here); an
                            // ancestor is a button that climbs back to its level.
                            let item = if is_current {
                                view! {
                                    <span class="c4-breadcrumb__crumb c4-breadcrumb__crumb--current"
                                        aria-current="page">
                                        {label}
                                    </span>
                                }.into_view()
                            } else {
                                view! {
                                    <button
                                        class="c4-breadcrumb__crumb"
                                        on:click=move |_| state.navigate_to_c4_crumb(depth)
                                    >
                                        {label}
                                    </button>
                                }.into_view()
                            };
                            view! {
                                <li class="c4-breadcrumb__item">
                                    {item}
                                </li>
                            }
                        })
                        .collect_view()}
                </ol>
            </nav>
        </Show>
    }
}
