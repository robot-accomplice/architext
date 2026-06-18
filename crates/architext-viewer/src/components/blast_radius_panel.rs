//! Blast Radius surface (DISPLAY only).
//!
//! A filterable focus-node selector (over the loaded architecture nodes) on the
//! left; on the right, the focused node's full reach as a card/chip grid — the
//! faithful render of the JS `BlastRadiusWorkspace`: a header (node name + type +
//! reach count) then conditional sections (only when non-empty): Depends on /
//! Depended on by (node chips colored by the single-source `--c4-*` role token),
//! Flows, Appears in views, Data handled (`--sens-*`), Decisions, Risks
//! (`--sev-*` badge), Owns files. Selecting a dependency/dependent chip
//! re-focuses; selecting a flow/view drives the matching mode selection.
//!
//! The reach is computed by the pure, native-tested `blast_radius` module; this
//! component only wires data → compute → DOM. Owned files require the live repo
//! file list (same `/api/repo-tree` source as Repo Tree), fetched on demand.
//!
//! Color is single-sourced: node-type chips via `diagram::role_color_var`, data
//! sensitivity via `--sens-*`, risk severity via `--sev-*` (never a role hue for
//! a severity/sensitivity signal). Section accents reuse those semantic scales
//! where the section IS semantic (Data → sensitivity, Risks → severity) and the
//! neutral `--accent` STATE token elsewhere — a deliberate, documented
//! divergence from the JS decorative per-section palette, to honor the
//! single-source / no-invented-hue mandate.

use leptos::*;
use leptos::spawn_local;

use crate::blast_radius::{blast_radius_for_node, BlastInputs, BlastRadius};
use crate::data::{fetch_repo_tree, models::RepoTreePayload, FetchError};
use crate::diagram::role_color_var;
use crate::repo_tree_model::FileEntry;
use crate::severity::{sensitivity_color_var, severity_color_var};
use crate::state::use_app_state;

/// The repo-tree fetch outcome (for owned files), held in a plain signal — same
/// pattern as the Repo Tree surface. `None` while loading.
type RepoState = Option<Result<RepoTreePayload, FetchError>>;

#[component]
pub fn BlastRadiusPanel() -> impl IntoView {
    let state = use_app_state();

    // The focused node id. Seed from the inspector's selected node if it is a
    // known node, else the first loaded node (so the surface is never empty).
    let focus = create_rw_signal::<Option<String>>(None);
    create_effect(move |_| {
        if focus.get_untracked().is_some() {
            return;
        }
        let data = state.data.get();
        let seed = state
            .selected_node
            .get_untracked()
            .filter(|id| data.nodes.iter().any(|n| &n.id == id))
            .or_else(|| data.nodes.first().map(|n| n.id.clone()));
        focus.set(seed);
    });

    // Search query over the node list.
    let query = create_rw_signal(String::new());

    // Live repo file list (for owned files), fetched once on mount.
    let repo = create_rw_signal::<RepoState>(None);
    spawn_local(async move {
        repo.set(Some(fetch_repo_tree().await));
    });

    // Re-focus to a node id (dependency/dependent chip click) + mirror it into
    // the inspector selection so the right panel follows.
    let refocus = Callback::new(move |id: String| {
        focus.set(Some(id.clone()));
        state.set_selected_node(id);
    });

    view! {
        <div class="blast-panel">
            // ── Focus-node selector ──────────────────────────────────────────
            <div class="blast-panel__picker">
                <div class="overline">"FOCUS NODE"</div>
                <input
                    class="blast-search"
                    r#type="text"
                    placeholder="Filter components…"
                    prop:value=move || query.get()
                    on:input=move |ev| query.set(event_target_value(&ev))
                />
                <div class="blast-panel__nodes">
                    {move || {
                        let data = state.data.get();
                        let q = query.get().trim().to_lowercase();
                        let active = focus.get();
                        data.nodes
                            .iter()
                            .filter(|n| {
                                q.is_empty()
                                    || n.name.to_lowercase().contains(&q)
                                    || n.id.to_lowercase().contains(&q)
                                    || n.node_type.to_lowercase().contains(&q)
                            })
                            .map(|n| node_option(
                                n.id.clone(),
                                n.name.clone(),
                                n.node_type.clone(),
                                active.as_deref() == Some(n.id.as_str()),
                                focus,
                                state,
                            ))
                            .collect_view()
                    }}
                </div>
            </div>

            // ── Reach grid for the focused node ──────────────────────────────
            <div class="blast-panel__reach">
                {move || {
                    let data = state.data.get();
                    let files: Vec<FileEntry> = repo
                        .get()
                        .and_then(|r| r.ok())
                        .map(|p| {
                            p.files
                                .into_iter()
                                .map(|f| FileEntry { path: f.path, size: f.size, mtime: f.mtime })
                                .collect()
                        })
                        .unwrap_or_default();

                    let radius = focus.get().and_then(|id| {
                        let input = BlastInputs {
                            nodes: &data.nodes,
                            flows: &data.flows,
                            decisions: &data.decisions,
                            risks: &data.risks,
                            data_classes: &data.data_classes,
                            views: &data.views,
                            files: &files,
                        };
                        blast_radius_for_node(&id, &input)
                    });

                    match radius {
                        Some(r) => reach_view(r, refocus, state).into_view(),
                        None => view! {
                            <p class="blast-panel__hint">
                                "Select a component on the left to see everything it reaches."
                            </p>
                        }.into_view(),
                    }
                }}
            </div>
        </div>
    }
}

/// One selectable node row in the focus picker. Left rail in the node's role
/// color; `--accent` STATE treatment when it is the active focus.
fn node_option(
    id: String,
    name: String,
    node_type: String,
    active: bool,
    focus: RwSignal<Option<String>>,
    state: crate::state::AppState,
) -> View {
    let rail = role_color_var(&node_type);
    let pick_id = id.clone();
    let on_click = move |_| {
        focus.set(Some(pick_id.clone()));
        state.set_selected_node(pick_id.clone());
    };
    view! {
        <button
            class="accent-surface blast-node-option"
            class:is-active=active
            style=format!("--accent:{rail}")
            on:click=on_click
        >
            <span class="blast-node-option__name">{name}</span>
            <span class="chip blast-node-option__type" style=format!("color:{rail}")>{node_type}</span>
        </button>
    }
    .into_view()
}

/// The reach grid: header + conditional sections, faithful to the JS workspace.
fn reach_view(r: BlastRadius, refocus: Callback<String>, state: crate::state::AppState) -> impl IntoView {
    let reach = r.reach_count();
    let node_rail = role_color_var(&r.node.node_type);
    let plural = if reach == 1 { "" } else { "s" };

    view! {
        <header class="blast-head accent-surface" style=format!("--accent:{node_rail}")>
            <div class="overline">{r.node.node_type.clone()}</div>
            <h2 class="blast-head__title">{r.node.name.clone()}</h2>
            <p class="blast-head__reach">
                {format!("Reaches {reach} element{plural} across the repository.")}
            </p>
        </header>

        <div class="blast-sections">
            // Depends on / Depended on by — node chips, role-colored, clickable.
            {node_section("Depends on", r.depends_on.clone(), refocus)}
            {node_section("Depended on by", r.dependents.clone(), refocus)}

            // Flows — clickable, switch to Flows mode focused on the flow.
            {(!r.flows.is_empty()).then(|| {
                let chips = r.flows.clone();
                view! {
                    <section class="blast-section accent-surface" style="--accent:var(--accent)">
                        {section_title("Flows", chips.len())}
                        <div class="blast-chips">
                            {chips.into_iter().map(|f| {
                                let fid = f.id.clone();
                                let on_click = move |_| select_flow(state, &fid);
                                view! {
                                    <button class="chip blast-chip" on:click=on_click>{f.name}</button>
                                }
                            }).collect_view()}
                        </div>
                    </section>
                }
            })}

            // Appears in views — clickable, switch to that view.
            {(!r.views.is_empty()).then(|| {
                let chips = r.views.clone();
                view! {
                    <section class="blast-section accent-surface" style="--accent:var(--accent)">
                        {section_title("Appears in views", chips.len())}
                        <div class="blast-chips">
                            {chips.into_iter().map(|v| {
                                let vid = v.id.clone();
                                let on_click = move |_| select_view(state, &vid);
                                view! {
                                    <button class="chip blast-chip" on:click=on_click>{v.name}</button>
                                }
                            }).collect_view()}
                        </div>
                    </section>
                }
            })}

            // Data handled — sensitivity-colored chips (its own --sens-* scale).
            {(!r.data_handled.is_empty()).then(|| {
                let chips = r.data_handled.clone();
                let accent = sensitivity_color_var(
                    chips.iter().filter_map(|d| d.sensitivity.as_deref()).max_by_key(sens_rank)
                );
                view! {
                    <section class="blast-section accent-surface" style=format!("--accent:{accent}")>
                        {section_title("Data handled", chips.len())}
                        <div class="blast-chips">
                            {chips.into_iter().map(|d| {
                                let tint = sensitivity_color_var(d.sensitivity.as_deref());
                                view! {
                                    <span class="chip blast-chip" style=format!("color:{tint}")>{d.name}</span>
                                }
                            }).collect_view()}
                        </div>
                    </section>
                }
            })}

            // Decisions — titles.
            {(!r.decisions.is_empty()).then(|| {
                let items = r.decisions.clone();
                view! {
                    <section class="blast-section accent-surface" style="--accent:var(--accent)">
                        {section_title("Decisions", items.len())}
                        <ul class="blast-list">
                            {items.into_iter().map(|d| view! { <li>{d.title}</li> }).collect_view()}
                        </ul>
                    </section>
                }
            })}

            // Risks — severity badge (its own --sev-* scale) + title.
            {(!r.risks.is_empty()).then(|| {
                let items = r.risks.clone();
                let accent = severity_color_var(
                    items.iter().filter_map(|rk| rk.severity.as_deref()).max_by_key(sev_rank)
                );
                view! {
                    <section class="blast-section accent-surface" style=format!("--accent:{accent}")>
                        {section_title("Risks", items.len())}
                        <ul class="blast-list">
                            {items.into_iter().map(|rk| {
                                let tint = severity_color_var(rk.severity.as_deref());
                                let sev = rk.severity.clone().unwrap_or_else(|| "?".to_string());
                                view! {
                                    <li class="blast-risk">
                                        <span class="chip blast-risk__sev" style=format!("color:{tint}")>{sev}</span>
                                        <span>{rk.title}</span>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    </section>
                }
            })}

            // Owns files — path + size.
            {(!r.owned_files.is_empty()).then(|| {
                let files = r.owned_files.clone();
                view! {
                    <section class="blast-section accent-surface" style="--accent:var(--accent)">
                        {section_title("Owns files", files.len())}
                        <ul class="blast-files">
                            {files.into_iter().map(|f| view! {
                                <li class="blast-file">
                                    <span class="mono blast-file__path">{f.path}</span>
                                    <span class="mono blast-file__size">{format_size(f.size)}</span>
                                </li>
                            }).collect_view()}
                        </ul>
                    </section>
                }
            })}
        </div>
    }
}

/// A counted section heading.
fn section_title(title: &'static str, count: usize) -> impl IntoView {
    view! {
        <h3 class="blast-section__title">
            {title}<span class="blast-count">{count}</span>
        </h3>
    }
}

/// A node-chip section (Depends on / Depended on by); empty → nothing rendered.
fn node_section(
    title: &'static str,
    nodes: Vec<crate::blast_radius::NodeRef>,
    refocus: Callback<String>,
) -> Option<View> {
    if nodes.is_empty() {
        return None;
    }
    let count = nodes.len();
    Some(
        view! {
            <section class="blast-section accent-surface" style="--accent:var(--accent)">
                {section_title(title, count)}
                <div class="blast-chips">
                    {nodes.into_iter().map(|n| {
                        let rail = role_color_var(&n.node_type);
                        let id = n.id.clone();
                        let on_click = move |_| refocus.call(id.clone());
                        view! {
                            <button
                                class="chip blast-chip blast-chip--node"
                                style=format!("color:{rail};border-color:{rail}")
                                title=format!("{} ({})", n.name, n.node_type)
                                on:click=on_click
                            >
                                {n.name}
                            </button>
                        }
                    }).collect_view()}
                </div>
            </section>
        }
        .into_view(),
    )
}

/// Switch to Flows mode and select the flow by id (chip → diagram).
fn select_flow(state: crate::state::AppState, flow_id: &str) {
    state.set_mode(crate::theme::Mode::Flows);
    let data = state.data.get_untracked();
    if let Some(idx) = data.flows.iter().position(|f| f.id == flow_id) {
        state.set_flow(idx);
    }
}

/// Switch to the view that contains the focused node (chip → diagram).
fn select_view(state: crate::state::AppState, view_id: &str) {
    let data = state.data.get_untracked();
    if let Some(idx) = data.views.iter().position(|v| v.id == view_id) {
        // Flows mode is the default flow-projection surface; the view selector
        // resolves a compatible flow.
        state.set_mode(crate::theme::Mode::Flows);
        state.set_view(idx);
    }
}

/// Ordinal rank for picking a section's headline sensitivity (high wins).
fn sens_rank(level: &&str) -> u8 {
    match *level {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

/// Ordinal rank for picking a section's headline severity (critical wins).
fn sev_rank(level: &&str) -> u8 {
    match *level {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

/// Human file size — faithful port of the JS `formatSize` (`repoTreeFormat.js`):
/// `null` → ""; `< 1024` → "{n} B"; else KB/MB/GB/TB with 1 dp under 10, rounded
/// at/above 10.
fn format_size(size: Option<u64>) -> String {
    let Some(bytes) = size else { return String::new() };
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let num = if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{}", value.round() as i64)
    };
    format!("{num} {}", UNITS[unit])
}
