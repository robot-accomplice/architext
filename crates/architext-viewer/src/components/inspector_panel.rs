//! Fixed-width right inspector.
//!
//! Data-bound metadata, selection-driven in every mode. A clicked diagram node
//! takes precedence and shows its type plus the relationships derived from the
//! loaded dataset — Depends on / Used by (node `dependencies` edges, read in
//! both directions), Data handled, and Appears-in-views. With nothing selected
//! the panel shows the current view (and, in flow modes, the flow) summary; a
//! genuinely diagram-less data mode (Rules / Release Truth) summarizes its set.
use leptos::*;

use crate::code_graph_graph::{format_signature, reach_badges, Reach};
use crate::code_graph_view_model::Tier;
use crate::components::data_risks_panel::DataRisksPanel;
use crate::components::notes_editor::NotesSection;
use crate::data::fetch_node_git;
use crate::data::models::{CodeGraphFunction, CodeGraphModule, DataClass, Node, NodeGit, View};
use crate::diagram::role_color_var;
use crate::release_truth::{release_path, release_tone, ReleaseDoc};
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

/// Everything the inspector renders for a selected code-graph FUNCTION (Plan C
/// Task 6): symbol, Go-style signature, `file:line`, fan-in/out, doc, and the
/// CANDIDATE reachability badges. The badges are exactly `reach_badges` output
/// — never filtered or relabelled — and each is rendered with
/// `Reach::tooltip()` verbatim as its hover text (it names the static-analysis
/// blind spots; a badge that reads as a verdict invites deleting live code).
/// Pure so it is unit-testable on native.
#[derive(Debug, PartialEq)]
struct CodeGraphFunctionDetail {
    symbol: String,
    signature: String,
    location: String,
    fan_in: u32,
    fan_out: u32,
    doc: Option<String>,
    badges: Vec<Reach>,
}

fn code_graph_function_detail(f: &CodeGraphFunction) -> CodeGraphFunctionDetail {
    CodeGraphFunctionDetail {
        symbol: f.symbol.clone(),
        signature: format_signature(&f.signature),
        location: format!("{}:{}", f.file, f.line),
        fan_in: f.fan_in,
        fan_out: f.fan_out,
        doc: f.doc.clone(),
        badges: reach_badges(f),
    }
}

/// The module-tier counterpart. Modules carry no per-module signature, doc, or
/// reachability flags, so the detail is the package plus its fan and the
/// contract's function counts — phrased as CANDIDATES, same as the badges.
#[derive(Debug, PartialEq)]
struct CodeGraphModuleDetail {
    pkg: String,
    fan_in: u32,
    fan_out: u32,
    functions: u32,
    dead_candidates: u32,
    test_only_candidates: u32,
}

fn code_graph_module_detail(m: &CodeGraphModule) -> CodeGraphModuleDetail {
    CodeGraphModuleDetail {
        pkg: m.pkg.clone(),
        fan_in: m.fan_in,
        fan_out: m.fan_out,
        functions: m.counts.functions,
        dead_candidates: m.counts.dead,
        test_only_candidates: m.counts.test_only,
    }
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
                        let source_paths = node.source_paths.clone();
                        let paths_csv = source_paths.join(",");
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
                            {(!source_paths.is_empty()).then(move || view! {
                                <div class="inspector__card">
                                    <div class="overline">"SOURCE"</div>
                                    {source_paths.iter().map(|p| view! {
                                        <p class="inspector__meta mono">{p.clone()}</p>
                                    }).collect_view()}
                                </div>
                            })}
                            {(!paths_csv.is_empty()).then(move || view! {
                                <NodeDevWindow paths=paths_csv/>
                            })}
                            <NotesSection
                                label="Node notes".to_string()
                                target_kind="node".to_string()
                                target_id=note_target
                            />
                        }.into_view();
                    }
                }

                // Code Graph: the WebGL canvas mirrors its selection into
                // `selected_code_graph_node` (a Magma id + tier — NEVER
                // `selected_node`, which is a different id-space). Resolve the
                // id against the matching tier's collection and render the
                // detail. An id that no longer resolves (e.g. an SSE reload
                // swapped the document under the selection) falls through to
                // the summary card below rather than dangling.
                if mode == Mode::CodeGraph {
                    if let Some(sel) = state.selected_code_graph_node.get() {
                        let cg = data.code_graph.as_ref().and_then(|r| r.as_ref().ok());
                        match (sel.tier, cg) {
                            (Tier::Functions, Some(cg)) => {
                                let f = cg
                                    .functions
                                    .as_ref()
                                    .and_then(|fs| fs.iter().find(|f| f.id == sel.id))
                                    .cloned();
                                if let Some(f) = f {
                                    let CodeGraphFunctionDetail {
                                        symbol,
                                        signature,
                                        location,
                                        fan_in,
                                        fan_out,
                                        doc,
                                        badges,
                                    } = code_graph_function_detail(&f);
                                    let clear = move |_| state.set_selected_code_graph_node(None);
                                    return view! {
                                        <button class="inspector__back" on:click=clear>
                                            "‹ back to code graph"
                                        </button>
                                        <div class="accent-surface inspector__card">
                                            <div class="overline">"FUNCTION"</div>
                                            <h2 class="inspector__title">{symbol}</h2>
                                            <p class="inspector__meta mono">{signature}</p>
                                            <p class="inspector__meta mono">{location}</p>
                                            <p class="inspector__meta">
                                                {format!("fan-in {fan_in} · fan-out {fan_out}")}
                                            </p>
                                            {doc.map(|d| view! { <p class="inspector__meta">{d}</p> })}
                                            {(!badges.is_empty()).then(move || view! {
                                                <div class="chip-row">
                                                    {badges.into_iter().map(|b| view! {
                                                        <span
                                                            class="chip chip--state cg-chip"
                                                            style=format!("color:{}", b.color_var())
                                                            title=b.tooltip()
                                                        >
                                                            {b.label()}
                                                        </span>
                                                    }).collect_view()}
                                                </div>
                                            })}
                                        </div>
                                    }
                                    .into_view();
                                }
                            }
                            (Tier::Modules, Some(cg)) => {
                                let m = cg
                                    .modules
                                    .as_ref()
                                    .and_then(|ms| ms.iter().find(|m| m.id == sel.id))
                                    .cloned();
                                if let Some(m) = m {
                                    let d = code_graph_module_detail(&m);
                                    let clear = move |_| state.set_selected_code_graph_node(None);
                                    return view! {
                                        <button class="inspector__back" on:click=clear>
                                            "‹ back to code graph"
                                        </button>
                                        <div class="accent-surface inspector__card">
                                            <div class="overline">"MODULE"</div>
                                            <h2 class="inspector__title">{d.pkg}</h2>
                                            <span class="chip">"module"</span>
                                            <p class="inspector__meta">
                                                {format!("fan-in {} · fan-out {}", d.fan_in, d.fan_out)}
                                            </p>
                                            <p class="inspector__meta">
                                                {format!(
                                                    "{} functions · {} dead-code candidates · {} test-only candidates",
                                                    d.functions, d.dead_candidates, d.test_only_candidates,
                                                )}
                                            </p>
                                        </div>
                                    }
                                    .into_view();
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // Data/Risks: the diagram renders in the center; the inspector
                // hosts the data-class + risk side panel (its own scales).
                if mode == Mode::DataRisks {
                    return view! { <DataRisksPanel/> }.into_view();
                }

                // Release Truth: a clicked Release Path item shows its detail
                // here (resolved through the same release_path the panel renders,
                // so the inspector and the path agree). Falls through to the
                // release count summary when nothing is selected.
                if mode == Mode::ReleaseTruth {
                    if let Some(item_id) = state.selected_release_item.get() {
                        let item = state
                            .selected_release
                            .get()
                            .and_then(|rid| {
                                data.release_details.iter().find(|d| d.id == rid).map(|d| d.raw.clone())
                            })
                            .and_then(|raw| ReleaseDoc::from_value(&raw))
                            .map(|doc| release_path(&doc))
                            .and_then(|path| {
                                path.into_iter().flat_map(|m| m.items).find(|i| i.id == item_id)
                            });
                        if let Some(it) = item {
                            let rail = release_tone_color_var(release_tone(it.status.as_deref()));
                            let clear = move |_| state.selected_release_item.set(None);
                            let meta: Vec<String> = [
                                it.kind.clone(),
                                it.priority.clone().map(|p| format!("{p} priority")),
                                it.owner.clone(),
                                (!it.workstream_name.is_empty()).then(|| it.workstream_name.clone()),
                                (!it.scope.is_empty()).then(|| it.scope.clone()),
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                            return view! {
                                <button class="inspector__back" on:click=clear>"‹ back to release"</button>
                                <div class="accent-surface inspector__card" style=format!("--accent:{rail}")>
                                    <div class="overline">"RELEASE ITEM"</div>
                                    <h2 class="inspector__title">{it.title.clone()}</h2>
                                    <span class="chip chip--state" style=format!("color:{rail}")>
                                        {it.line_state.clone()}
                                    </span>
                                    {it.summary.clone().map(|s| view! { <p class="inspector__meta">{s}</p> })}
                                    {(!meta.is_empty()).then(|| view! {
                                        <p class="inspector__meta mono">{meta.join(" · ")}</p>
                                    })}
                                    {it.blocked_by.clone().map(|b| view! {
                                        <p class="release-path-blockers">{format!("Blocked by: {b}")}</p>
                                    })}
                                </div>
                            }
                            .into_view();
                        }
                    }
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
                    Mode::CodeGraph => Some((
                        "Code graph",
                        data.code_graph.as_ref()
                            .and_then(|r| r.as_ref().ok())
                            .and_then(|cg| cg.modules.as_ref())
                            .map(|m| m.len())
                            .unwrap_or(0),
                        data.code_graph.as_ref()
                            .and_then(|r| r.as_ref().ok())
                            .map(|cg| cg.module.clone()),
                    )),
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

/// Trim an ISO datetime to its `YYYY-MM-DD` date for display.
fn short_date(iso: &str) -> String {
    iso.get(..10).unwrap_or(iso).to_string()
}

/// Human "active span" between two ISO datetimes (parsed via the JS `Date`), or
/// `None` when either is missing/unparseable. Days under a year, else years.
fn active_span(first: Option<&str>, last: Option<&str>) -> Option<String> {
    let (f, l) = (first?, last?);
    let fms = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(f)).get_time();
    let lms = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(l)).get_time();
    if fms.is_nan() || lms.is_nan() {
        return None;
    }
    let days = ((lms - fms) / 86_400_000.0).round().max(0.0) as i64;
    Some(if days >= 365 {
        format!("{:.1} years", days as f64 / 365.0)
    } else if days <= 1 {
        "under a day".to_string()
    } else {
        format!("{days} days")
    })
}

/// The git "development window" for a node, fetched on demand from
/// `/api/node-git` for its `sourcePaths`. Renders nothing while loading or when
/// the paths are untracked (the sanitized corpus) — supplementary, never blank.
/// Uses a plain signal + `spawn_local` (not a `Resource`/`Suspense`, which would
/// conflict with the App-level data Suspense — mirrors the Repo Tree fetch).
#[component]
fn NodeDevWindow(paths: String) -> impl IntoView {
    let git = create_rw_signal::<Option<NodeGit>>(None);
    spawn_local(async move {
        git.set(fetch_node_git(&paths).await);
    });
    move || {
        git.get().filter(|g| g.tracked).map(|g| {
            let first = g.first_commit.clone().map(|d| short_date(&d)).unwrap_or_default();
            let last = g.last_commit.clone().map(|d| short_date(&d)).unwrap_or_default();
            let span = active_span(g.first_commit.as_deref(), g.last_commit.as_deref());
            let commits = g.commit_count.unwrap_or(0);
            let authors = g.authors.join(", ");
            view! {
                <div class="accent-surface inspector__card">
                    <div class="overline">"DEVELOPMENT"</div>
                    <p class="inspector__meta">"First seen "<span class="mono">{first}</span></p>
                    <p class="inspector__meta">"Last changed "<span class="mono">{last}</span></p>
                    {span.map(|d| view! {
                        <p class="inspector__meta">{format!("Active span: {d}")}</p>
                    })}
                    <p class="inspector__meta">{format!("{commits} commits")}</p>
                    {(!authors.is_empty()).then(|| view! {
                        <p class="inspector__meta">{format!("Contributors: {authors}")}</p>
                    })}
                </div>
            }
        })
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

    // --- Code Graph detail (Task 6) ------------------------------------------

    fn function(flags: serde_json::Value) -> CodeGraphFunction {
        let mut v = serde_json::json!({
            "id": "f1", "symbol": "srv.handle", "pkg": "srv", "file": "h.go", "line": 42,
            "kind": "func", "exported": true, "test": false, "root": false,
            "generated": false, "reachable": true, "prod_reachable": true,
            "signature": {"params": [{"name": "a", "type": "int"}], "results": [{"type": "error"}]},
            "doc": "Handle serves one request.",
            "fan_in": 3, "fan_out": 2
        });
        if let (Some(base), Some(over)) = (v.as_object_mut(), flags.as_object()) {
            for (k, val) in over {
                base.insert(k.clone(), val.clone());
            }
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn function_detail_assembles_the_display_facts() {
        let d = code_graph_function_detail(&function(serde_json::json!({})));
        assert_eq!(d.symbol, "srv.handle");
        assert_eq!(d.signature, "(a int) error");
        assert_eq!(d.location, "h.go:42");
        assert_eq!((d.fan_in, d.fan_out), (3, 2));
        assert_eq!(d.doc.as_deref(), Some("Handle serves one request."));
        assert!(d.badges.is_empty(), "a plain prod-reachable function earns no badge");
    }

    #[test]
    fn function_detail_badges_are_exactly_reach_badges_with_verbatim_tooltips() {
        // WHY: dead/test-only are static-analysis CANDIDATES. The inspector
        // must render `reach_badges` output unfiltered, and every badge's
        // hover text is `Reach::tooltip()` verbatim — the inferred ones name
        // the blind spots so the badge never reads as a verdict.
        let dead = function(serde_json::json!({"reachable": false, "prod_reachable": false}));
        let d = code_graph_function_detail(&dead);
        assert_eq!(d.badges, reach_badges(&dead), "no filtering/relabeling on the way out");
        assert_eq!(d.badges, vec![Reach::Dead]);
        let t = Reach::Dead.tooltip();
        assert!(t.starts_with("CANDIDATE"), "tooltip is the candidate warning: {t}");
        assert!(t.contains("Reflection"), "tooltip names a blind spot: {t}");
    }

    #[test]
    fn module_detail_carries_fan_and_candidate_counts() {
        let m: CodeGraphModule = serde_json::from_value(serde_json::json!({
            "id": "example.com/x/srv", "pkg": "srv",
            "counts": {"functions": 9, "dead": 2, "test_only": 1},
            "fan_in": 4, "fan_out": 5
        }))
        .unwrap();
        let d = code_graph_module_detail(&m);
        assert_eq!(d.pkg, "srv");
        assert_eq!((d.fan_in, d.fan_out), (4, 5));
        assert_eq!((d.functions, d.dead_candidates, d.test_only_candidates), (9, 2, 1));
    }
}
