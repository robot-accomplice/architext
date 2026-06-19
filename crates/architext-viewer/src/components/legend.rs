//! Canvas legend overlay — colors encode TYPE, so spell that out.
//!
//! A compact, collapsible panel pinned bottom-left of the canvas. It lists ONLY
//! what the current view renders: each node TYPE present (its `--c4-*` swatch +
//! the type glyph + the type name), and — when structural edges are shown — each
//! relationship KIND present (its glyph + word). It never lists a type or kind
//! absent from the view (the caller derives the rows via `diagram::svg::
//! legend_for`). On-language: overline header, hairline, mono, tiered black;
//! the collapse toggle is the only state, expressed via `--accent` on hover.

use leptos::*;

use crate::components::node_icon::{node_icon_path, node_type_label};
use crate::components::relationship_icon::RelationshipKind;
use crate::diagram::role_color_var;
use crate::diagram::svg::LegendModel;

/// The canvas legend. `model` carries the present node types + relationship
/// kinds; an empty model renders nothing. `collapsed` is the open/closed state
/// (owned by the canvas), toggled via `on_toggle`.
#[component]
pub fn Legend(
    #[prop(into)] model: Signal<LegendModel>,
    #[prop(into)] collapsed: Signal<bool>,
    #[prop(into)] on_toggle: Callback<()>,
) -> impl IntoView {
    // Nothing present → no legend (don't render an empty panel).
    let has_content = move || {
        model.with(|m| !m.node_types.is_empty() || !m.relationship_kinds.is_empty())
    };

    let node_rows = move || {
        model.with(|m| {
            m.node_types
                .iter()
                .map(|ty| {
                    let role = role_color_var(ty);
                    let glyph = node_icon_path(ty);
                    let name = node_type_label(ty);
                    view! {
                        <li class="legend__row">
                            <span class="legend__swatch" style=("background", role.clone())></span>
                            <svg
                                class="legend__glyph"
                                viewBox="0 0 24 24"
                                aria-hidden="true"
                                style=("color", role)
                            >
                                <path d=glyph></path>
                            </svg>
                            <span class="legend__name">{name}</span>
                        </li>
                    }
                })
                .collect_view()
        })
    };

    let rel_rows = move || {
        model.with(|m| {
            m.relationship_kinds
                .iter()
                .map(|kind: &RelationshipKind| {
                    view! {
                        <li class="legend__row">
                            <svg class="legend__glyph legend__glyph--rel" viewBox="0 0 24 24" aria-hidden="true">
                                <path d=kind.icon_path()></path>
                            </svg>
                            <span class="legend__name">{kind.word()}</span>
                        </li>
                    }
                })
                .collect_view()
        })
    };

    let has_rels = move || model.with(|m| !m.relationship_kinds.is_empty());

    view! {
        <Show when=has_content>
            <div class=move || if collapsed.get() { "legend legend--collapsed" } else { "legend" }>
                <button
                    class="legend__header"
                    title=move || if collapsed.get() { "Show legend" } else { "Hide legend" }
                    on:click=move |_| on_toggle.call(())
                >
                    <span class="overline">"LEGEND"</span>
                    <span class="legend__toggle">
                        {move || if collapsed.get() { "+" } else { "−" }}
                    </span>
                </button>
                <Show when=move || !collapsed.get()>
                    <div class="legend__body">
                        <ul class="legend__list">{node_rows}</ul>
                        <Show when=has_rels>
                            <div class="legend__divider"></div>
                            <ul class="legend__list">{rel_rows}</ul>
                        </Show>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
