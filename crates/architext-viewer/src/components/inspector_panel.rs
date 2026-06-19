//! Fixed-width right inspector.
//!
//! Data-bound metadata, selection-driven in every mode. A clicked diagram node
//! takes precedence and shows its type plus the relationships derived from the
//! loaded dataset — Depends on / Used by (node `dependencies` edges, read in
//! both directions), Data handled, and Appears-in-views. With nothing selected
//! the panel shows the current view (and, in flow modes, the flow) summary; a
//! genuinely diagram-less data mode (Rules / Release Truth) summarizes its set.
use leptos::*;

use crate::components::data_risks_panel::DataRisksPanel;
use crate::components::notes_editor::NotesSection;
use crate::data::models::{DataClass, Node, View};
use crate::diagram::role_color_var;
use crate::release_truth::release_tone;
use crate::severity::release_tone_color_var;
use crate::state::use_app_state;
use crate::theme::Mode;

/// The relationships and cross-references shown for a selected node, all derived
/// from the loaded `ArchitectureData` (nodes + views + data-classes). Names, not
/// ids, so the panel is readable; computed once per selection.
#[derive(Debug, Default, PartialEq)]
struct NodeRelations {
    /// Nodes this node points at via its own `dependencies` (outgoing edges).
    depends_on: Vec<String>,
    /// Nodes whose `dependencies` name this node (incoming edges).
    used_by: Vec<String>,
    /// Data classes this node handles — resolved to class names where the id
    /// matches a known class, else the raw id (so unmapped ids still surface).
    data_handled: Vec<String>,
    /// Views whose lanes include this node, by view name.
    appears_in: Vec<String>,
}

/// Resolve a node id to its display name, falling back to the id when unknown.
fn node_name(nodes: &[Node], id: &str) -> String {
    nodes.iter().find(|n| n.id == id).map(|n| n.name.clone()).unwrap_or_else(|| id.to_string())
}

/// Derive a node's relationships from the dataset. `depends_on` is the node's
/// own `dependencies` (outgoing); `used_by` is the reverse edge set (every node
/// that lists this id in its `dependencies`); `data_handled` resolves the node's
/// `dataHandled` ids to data-class names; `appears_in` lists the views whose
/// lanes contain the node. Pure so it is unit-testable on native.
fn derive_node_relations(
    nodes: &[Node],
    views: &[View],
    data_classes: &[DataClass],
    node: &Node,
) -> NodeRelations {
    let depends_on = node.dependencies.iter().map(|id| node_name(nodes, id)).collect();

    let used_by = nodes
        .iter()
        .filter(|n| n.id != node.id && n.dependencies.iter().any(|d| d == &node.id))
        .map(|n| n.name.clone())
        .collect();

    let data_handled = node
        .data_handled
        .iter()
        .map(|id| {
            data_classes
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.clone())
        })
        .collect();

    let appears_in = views
        .iter()
        .filter(|v| v.lanes.iter().any(|l| l.node_ids.iter().any(|id| id == &node.id)))
        .map(|v| v.name.clone())
        .collect();

    NodeRelations { depends_on, used_by, data_handled, appears_in }
}

/// A labeled chip group: an `.overline` label with a count, then the values as
/// chips. Renders nothing when empty so absent relationships don't leave a dead
/// label. `chip_color` optionally tints the chips (used for the data-handled
/// group); `None` leaves the default chip tone.
fn chip_group(label: &str, values: Vec<String>, chip_color: Option<&str>) -> Option<leptos::View> {
    if values.is_empty() {
        return None;
    }
    let label = format!("{label} · {}", values.len());
    let color = chip_color.map(str::to_string);
    let chips = values
        .into_iter()
        .map(move |v| {
            let style = color.clone().map(|c| format!("color:{c}"));
            view! { <span class="chip" style=style>{v}</span> }
        })
        .collect_view();
    Some(
        view! {
            <div class="inspector__rel">
                <div class="overline">{label}</div>
                <div class="chip-row">{chips}</div>
            </div>
        }
        .into_view(),
    )
}

#[component]
pub fn InspectorPanel() -> impl IntoView {
    let state = use_app_state();
    let collapsed = state.inspector_collapsed;
    let toggle = move |_| collapsed.update(|c| *c = !*c);

    let aside_class = move || {
        if collapsed.get() {
            "inspector inspector--collapsed"
        } else {
            "inspector"
        }
    };

    let body = move || {
                let data = state.data.get();
                let mode = state.mode.get();

                // A clicked diagram node takes precedence: show its type plus its
                // derived relationships. The type chip carries its single-source
                // --c4-{type} role color (identity, not state); relationship chips
                // are neutral, with data-handled tinted on the data-class scale.
                if let Some(node_id) = state.selected_node.get() {
                    if let Some(node) = data.nodes.iter().find(|n| n.id == node_id).cloned() {
                        let role = role_color_var(&node.node_type);
                        let note_target = node.id.clone();
                        let rel = derive_node_relations(
                            &data.nodes,
                            &data.views,
                            &data.data_classes,
                            &node,
                        );
                        let clear = move |_| state.selected_node.set(None);
                        return view! {
                            <button class="inspector__back" on:click=clear>
                                "‹ back to view"
                            </button>
                            <div class="accent-surface inspector__card">
                                <div class="overline">"NODE"</div>
                                <h2 class="inspector__title">{node.name.clone()}</h2>
                                <span class="chip" style=format!("color:{role}")>
                                    {node.node_type.clone()}
                                </span>
                                {node.summary.clone().map(|s| view! {
                                    <p class="inspector__meta">{s}</p>
                                })}
                                {node.owner.clone().map(|o| view! {
                                    <p class="inspector__meta">{format!("Owner: {o}")}</p>
                                })}
                            </div>
                            {chip_group("Depends on", rel.depends_on, None)}
                            {chip_group("Used by", rel.used_by, None)}
                            {chip_group(
                                "Data handled",
                                rel.data_handled,
                                Some("var(--sens-medium)"),
                            )}
                            {chip_group("Appears in views", rel.appears_in, None)}
                            <NotesSection
                                label="Node notes".to_string()
                                target_kind="node".to_string()
                                target_id=note_target
                            />
                        }.into_view();
                    }
                }

                // Data/Risks: the diagram renders in the center; the inspector
                // hosts the data-class + risk side panel (its own scales).
                if mode == Mode::DataRisks {
                    return view! { <DataRisksPanel/> }.into_view();
                }

                // Genuinely diagram-less data modes summarize their own set and
                // add a one-liner inviting node selection where it applies. The
                // node-bearing diagram modes (C4, Deployment, Blast Radius, Repo
                // Tree) instead fall through to the view + flow metadata card
                // below so nothing-selected still shows the current context, and a
                // node click drives the panel through the branch above.
                let summary_card = match mode {
                    Mode::Rules => Some(("Rules", data.rules.len(),
                        data.rules.first().map(|r| r.title.clone()))),
                    Mode::ReleaseTruth => Some(("Releases", data.release_index.as_ref()
                        .map(|r| r.releases.len()).unwrap_or(0),
                        data.release_index.as_ref()
                            .and_then(|r| r.current_release_id.clone()))),
                    _ => None,
                };
                if let Some((label, count, sample)) = summary_card {
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

                // View (+ flow) metadata. Flow-projecting and node-bearing diagram
                // modes alike land here when nothing is selected.
                let view = state.view_idx.get().and_then(|i| data.views.get(i).cloned());
                let flow = state.flow_idx.get().and_then(|i| data.flows.get(i).cloned());

                // Diagram modes with clickable nodes (no flow selector) invite the
                // node-driven panel; flow-projecting modes already show the flow.
                let selectable = mode.has_clickable_nodes() && !mode.projects_flows();

                view! {
                    {view.map(|v| {
                        let view_id = v.id.clone();
                        view! {
                            <div class="accent-surface inspector__card">
                                <div class="overline">"VIEW"</div>
                                <h2 class="inspector__title">{v.name.clone()}</h2>
                                <span class="chip">{v.view_type.clone()}</span>
                                {v.summary.clone().map(|s| view! {
                                    <p class="inspector__meta">{s}</p>
                                })}
                            </div>
                            <NotesSection
                                label="View notes".to_string()
                                target_kind="view".to_string()
                                target_id=view_id
                            />
                        }
                    })}
                    {flow.map(|f| {
                        let flow_id = f.id.clone();
                        // Flow status is a STATE signal: tint on the release-tone
                        // scale (never a --c4-* role hue), DESIGN.md.
                        let status_chip = f.status.clone().map(|s| {
                            let tone = release_tone_color_var(release_tone(Some(&s)));
                            view! { <span class="chip" style=format!("color:{tone}")>{s}</span> }
                        });
                        view! {
                            <div class="accent-surface inspector__card">
                                <div class="overline">"FLOW"</div>
                                <h2 class="inspector__title">{f.name.clone()}</h2>
                                {status_chip}
                                {f.summary.clone().map(|s| view! {
                                    <p class="inspector__meta">{s}</p>
                                })}
                                {f.trigger.clone().map(|t| view! {
                                    <p class="inspector__meta">{format!("Trigger: {t}")}</p>
                                })}
                            </div>
                            <NotesSection
                                label="Flow notes".to_string()
                                target_kind="flow".to_string()
                                target_id=flow_id
                            />
                        }
                    })}
                    {selectable.then(|| view! {
                        <p class="inspector__hint">
                            "Select a node on the canvas to inspect its connections."
                        </p>
                    })}
                }.into_view()
    };

    view! {
        <aside class=aside_class>
            <Show
                when=move || collapsed.get()
                fallback=move || view! {
                    // Inspector mirrors the nav: its collapse toggle hugs the
                    // central canvas — here the inspector's LEFT edge (the
                    // canvas↔inspector boundary). The header reverses order so the
                    // chevron is left-aligned and the label trails it.
                    <div class="panel-collapse-header">
                        <button
                            class="panel-collapse-toggle"
                            title="Collapse inspector"
                            on:click=toggle
                        >"›"</button>
                        <div class="overline inspector__section-label">"INSPECTOR"</div>
                    </div>
                    {body()}
                }
            >
                <button
                    class="panel-collapse-toggle panel-collapse-toggle--rail-left"
                    title="Expand inspector"
                    on:click=toggle
                >"‹"</button>
            </Show>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the test dataset from JSON so fixtures exercise the real serde
    /// shapes (dependencies, dataHandled, view lanes) the derivation reads.
    fn nodes() -> Vec<Node> {
        serde_json::from_value(serde_json::json!([
            { "id": "cli", "type": "service", "name": "Architext CLI",
              "dependencies": ["validator", "store"], "dataHandled": ["arch-model", "raw-id"] },
            { "id": "validator", "type": "module", "name": "Schema validator",
              "dependencies": ["store"] },
            { "id": "store", "type": "data-store", "name": "Target data files" },
            { "id": "unrelated", "type": "module", "name": "Routing engine" }
        ]))
        .unwrap()
    }

    fn views() -> Vec<View> {
        serde_json::from_value(serde_json::json!([
            { "id": "v1", "name": "System Map", "type": "system-map",
              "lanes": [{ "id": "l1", "nodeIds": ["cli", "store"] }] },
            { "id": "v2", "name": "Dataflow", "type": "dataflow",
              "lanes": [{ "id": "l2", "nodeIds": ["cli"] }] },
            { "id": "v3", "name": "Rules", "type": "rules", "lanes": [] }
        ]))
        .unwrap()
    }

    fn data_classes() -> Vec<DataClass> {
        serde_json::from_value(serde_json::json!([
            { "id": "arch-model", "name": "Architecture model" }
        ]))
        .unwrap()
    }

    fn node_by_id<'a>(nodes: &'a [Node], id: &str) -> &'a Node {
        nodes.iter().find(|n| n.id == id).unwrap()
    }

    #[test]
    fn depends_on_resolves_node_dependencies_to_names() {
        let nodes = nodes();
        let rel =
            derive_node_relations(&nodes, &views(), &data_classes(), node_by_id(&nodes, "cli"));
        assert_eq!(rel.depends_on, vec!["Schema validator", "Target data files"]);
    }

    #[test]
    fn used_by_is_the_reverse_edge_set() {
        let nodes = nodes();
        // `store` is depended on by both `cli` and `validator`.
        let rel =
            derive_node_relations(&nodes, &views(), &data_classes(), node_by_id(&nodes, "store"));
        assert_eq!(rel.used_by, vec!["Architext CLI", "Schema validator"]);
        assert!(rel.depends_on.is_empty());
    }

    #[test]
    fn data_handled_resolves_known_ids_and_keeps_unknown_raw() {
        let nodes = nodes();
        let rel =
            derive_node_relations(&nodes, &views(), &data_classes(), node_by_id(&nodes, "cli"));
        // Known id → class name; unmapped id surfaces as the raw id.
        assert_eq!(rel.data_handled, vec!["Architecture model", "raw-id"]);
    }

    #[test]
    fn appears_in_lists_views_whose_lanes_contain_the_node() {
        let nodes = nodes();
        let rel =
            derive_node_relations(&nodes, &views(), &data_classes(), node_by_id(&nodes, "cli"));
        assert_eq!(rel.appears_in, vec!["System Map", "Dataflow"]);
    }

    #[test]
    fn node_with_no_relationships_is_all_empty() {
        let nodes = nodes();
        let rel = derive_node_relations(
            &nodes,
            &views(),
            &data_classes(),
            node_by_id(&nodes, "unrelated"),
        );
        assert_eq!(rel, NodeRelations::default());
    }
}
