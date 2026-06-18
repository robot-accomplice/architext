//! Read-only diagram-config panel (a right-side drawer over the shell).
//!
//! Displays the resolved diagram config from `/api/config` (already loaded into
//! `AppState.data.config.diagram`) so the maintainer can see the layout /
//! sequence / zoom / legibility values in effect. READ-ONLY this slice — editing
//! and `POST /api/config` are a later slice (config WRITE is V5). The drawer is
//! opened/closed by an `open` signal owned by the header.

use leptos::*;
use serde_json::Value;

use crate::state::use_app_state;

/// Flatten the resolved `diagram` config object into ordered `(path, value)`
/// rows for display. Nested objects (e.g. `layout`, `sequence`, `zoom`) prefix
/// their keys with the section name so the grouping the server emits stays
/// legible (`layout.nodeGap`, `sequence.actorWidth`, ...). Non-object configs
/// degrade to a single `(key, scalar)` row.
fn flatten_config(value: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten_config(v, &path, out);
            }
        }
        Value::String(s) => out.push((prefix.to_string(), s.clone())),
        Value::Null => out.push((prefix.to_string(), "null".to_string())),
        other => out.push((prefix.to_string(), other.to_string())),
    }
}

#[component]
pub fn ConfigPanel(
    /// Whether the drawer is open. Owned by the header; the close button writes it.
    #[prop(into)] open: RwSignal<bool>,
) -> impl IntoView {
    let state = use_app_state();

    let rows = move || {
        let data = state.data.get();
        let mut out = Vec::new();
        if let Some(cfg) = data.config.as_ref() {
            flatten_config(&cfg.diagram, "", &mut out);
        }
        out
    };

    view! {
        <Show when=move || open.get() fallback=|| ()>
            // Scrim closes the drawer on click-away.
            <div class="config-scrim" on:click=move |_| open.set(false)></div>
            <aside class="config-drawer" role="dialog" aria-label="Diagram configuration">
                <div class="config-drawer__head">
                    <div>
                        <div class="overline">"DIAGRAM CONFIG"</div>
                        <h2 class="config-drawer__title">"Resolved configuration"</h2>
                    </div>
                    <button
                        class="panel-collapse-toggle"
                        title="Close config"
                        on:click=move |_| open.set(false)
                    >"✕"</button>
                </div>
                <p class="config-drawer__note">
                    "Read-only — values resolved from /api/config. Editing arrives in a later pass."
                </p>
                {move || {
                    let rows = rows();
                    if rows.is_empty() {
                        view! {
                            <p class="config-drawer__empty">"No diagram config resolved."</p>
                        }.into_view()
                    } else {
                        view! {
                            <dl class="config-list">
                                {rows.into_iter().map(|(k, v)| view! {
                                    <div class="config-list__row">
                                        <dt class="config-list__key mono">{k}</dt>
                                        <dd class="config-list__val mono">{v}</dd>
                                    </div>
                                }).collect_view()}
                            </dl>
                        }.into_view()
                    }
                }}
            </aside>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_prefixes_nested_sections_and_orders_scalars() {
        let cfg = json!({
            "layout": { "nodeGap": 48, "rankGap": 80 },
            "zoom": { "min": 0.1, "max": 4 },
            "legibility": "high"
        });
        let mut out = Vec::new();
        flatten_config(&cfg, "", &mut out);
        // Nested keys are section-prefixed; scalars carry through.
        assert!(out.contains(&("layout.nodeGap".to_string(), "48".to_string())));
        assert!(out.contains(&("layout.rankGap".to_string(), "80".to_string())));
        assert!(out.contains(&("zoom.min".to_string(), "0.1".to_string())));
        assert!(out.contains(&("legibility".to_string(), "high".to_string())));
    }

    #[test]
    fn flatten_empty_config_yields_no_rows() {
        let mut out = Vec::new();
        flatten_config(&Value::Null, "", &mut out);
        // A null top-level config is a single null row, not a crash; an empty
        // object yields nothing.
        assert_eq!(out, vec![("".to_string(), "null".to_string())]);
        let mut out2 = Vec::new();
        flatten_config(&json!({}), "", &mut out2);
        assert!(out2.is_empty());
    }
}
