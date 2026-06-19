//! Release Truth surface (DISPLAY only).
//!
//! The center column is the selected release's reviewed truth: a compact summary
//! header (name / version / status / posture / target window), the Release Path
//! (milestone → item progression — the faithful port of the JS `ReleasePath`),
//! and the workstreams list with progress. Read-only — planning / editing is V5.
//!
//! The release SELECTOR lives in the left nav's southern slot (`SelectorBar`,
//! Release Truth mode), mirroring how Flows mode renders its FLOW/VIEW selectors
//! there. Both bind to the shared `state.selected_release` signal, which this
//! panel seeds (currentReleaseId / newest) on first render — single source of
//! truth, so picking a release in the nav updates this detail.
//!
//! The detail document is parsed by the pure, native-tested `release_truth`
//! module (`ReleaseDoc::from_value` over the already-loaded `release_details`
//! raw JSON); this component only wires data → shape → DOM.
//!
//! DESIGN.md fidelity: status / posture color via the state/severity scale
//! (`severity::release_tone_color_var`, never a `--c4-*` role hue); cards are
//! `.accent-surface`, section labels `.overline`, versions/dates `mono`. The
//! Kanban board + trend chart are DEFERRED to a later slice (and ultimately V5
//! editing): the Release Path is the primary, faithful projection of the
//! milestone/scope progression, and shipping it first keeps this slice focused
//! on the read-only path rather than duplicating the same facts in three views.

use leptos::*;

use crate::components::release_planning::ReleasePlanningEditor;
use crate::components::release_trend_chart::ReleaseTrendChart;
use crate::release_truth::{release_path, release_tone, MilestoneView, PathItem, ReleaseDoc};
use crate::severity::release_tone_color_var;
use crate::state::use_app_state;

#[component]
pub fn ReleaseTruthPanel() -> impl IntoView {
    let state = use_app_state();

    // The selected release id — SHARED with the left-nav RELEASE selector
    // (`SelectorBar`), so picking there drives this detail. Seed from the index's
    // currentReleaseId, else the last release summary (the newest).
    let selected = state.selected_release;
    create_effect(move |_| {
        if selected.get_untracked().is_some() {
            return;
        }
        let data = state.data.get();
        let seed = data.release_index.as_ref().and_then(|idx| {
            idx.current_release_id
                .clone()
                .or_else(|| idx.releases.last().map(|r| r.id.clone()))
        });
        selected.set(seed);
    });

    // Planning mode toggle (JS `planningMode`). Reset whenever the selected
    // release changes so a non-editable selection never lands in the editor.
    let planning_mode = create_rw_signal(false);
    create_effect(move |_| {
        selected.get();
        planning_mode.set(false);
        // A path-item selection from the prior release must not linger.
        state.selected_release_item.set(None);
    });

    // The historical trend chart is docked at the bottom (fixed), with the
    // release detail scrolling independently above it. The dock is minimizable
    // so the reader can reclaim the vertical space for the detail.
    let history_minimized = create_rw_signal(false);

    view! {
        <div class="release-panel">
            // ── Selected release truth (or the planning editor) ──────────────
            // The release SELECTOR lives in the left nav (SelectorBar); this
            // column holds only the detail for the shared selection, and SCROLLS
            // independently of the fixed history dock below.
            <div class="release-panel__detail">
                {move || {
                    let data = state.data.get();
                    let Some(id) = selected.get() else {
                        return view! {
                            <p class="release-panel__hint">"Select a release to see its truth."</p>
                        }.into_view();
                    };
                    // The selected summary decides whether planning is offered
                    // (JS `canEditRelease = status !== "completed"`).
                    let summary = data
                        .release_index
                        .as_ref()
                        .and_then(|idx| idx.releases.iter().find(|r| r.id == id).cloned());
                    let can_edit = summary
                        .as_ref()
                        .map(|s| s.status.as_deref() != Some("completed"))
                        .unwrap_or(false);

                    let raw = data.release_details.iter().find(|d| d.id == id).map(|d| &d.raw);
                    let Some(raw) = raw else {
                        return view! {
                            <p class="release-panel__hint">
                                {format!("No detail document loaded for {id}.")}
                            </p>
                        }.into_view();
                    };
                    let Some(doc) = ReleaseDoc::from_value(raw) else {
                        // FAIL LOUD: an unparseable detail surfaces a clear error,
                        // never a silently-empty path.
                        return view! {
                            <p class="release-panel__hint release-panel__hint--error">
                                {format!("Release detail for {id} did not match the expected shape.")}
                            </p>
                        }.into_view();
                    };

                    let edit_toggle = can_edit.then(|| {
                        view! {
                            <button class="release-panel__edit-toggle" type="button"
                                on:click=move |_| planning_mode.update(|v| *v = !*v)
                            >
                                {move || if planning_mode.get() { "View truth" } else { "Edit plan" }}
                            </button>
                        }
                    });

                    // Planning editor when toggled on AND the release is editable;
                    // otherwise the read-only truth view.
                    let versions: Vec<Option<String>> = data
                        .release_index
                        .as_ref()
                        .map(|idx| idx.releases.iter().map(|r| r.version.clone()).collect())
                        .unwrap_or_default();
                    let body = if can_edit && planning_mode.get() {
                        view! {
                            <ReleasePlanningEditor
                                versions=versions
                                active_release_id=Some(id.clone())
                                active_detail=Some(doc.clone())
                                on_done=Callback::new(move |_| planning_mode.set(false))
                            />
                        }
                        .into_view()
                    } else {
                        release_detail_view(doc).into_view()
                    };

                    view! {
                        {edit_toggle}
                        {body}
                    }
                    .into_view()
                }}
            </div>

            // ── Historical trend chart — FIXED bottom dock, minimizable ──────
            <div
                class="release-history-dock"
                class=("release-history-dock--min", move || history_minimized.get())
            >
                <div class="release-history-dock__head">
                    <span class="overline">"RELEASE HISTORY"</span>
                    <button
                        class="release-history-dock__toggle mono"
                        type="button"
                        on:click=move |_| history_minimized.update(|v| *v = !*v)
                    >
                        {move || if history_minimized.get() { "▴ Show chart" } else { "▾ Hide chart" }}
                    </button>
                </div>
                {move || (!history_minimized.get()).then(|| view! { <ReleaseTrendChart/> })}
            </div>
        </div>
    }
}

/// The selected release's header + Release Path + workstreams.
fn release_detail_view(doc: ReleaseDoc) -> impl IntoView {
    let status_rail = release_tone_color_var(release_tone(doc.status.as_deref()));
    let posture_rail = release_tone_color_var(release_tone(doc.posture.as_deref()));
    let progress = doc.progress();
    let path = release_path(&doc);

    let name = doc.name.clone().unwrap_or_else(|| doc.id.clone());
    let version = doc.version.clone().unwrap_or_default();
    let target = doc
        .target_date
        .clone()
        .or_else(|| doc.target_window.clone())
        .unwrap_or_default();
    let status = doc.status.clone().unwrap_or_else(|| "unknown".to_string());
    let posture = doc.posture.clone().unwrap_or_else(|| "unknown".to_string());

    view! {
        // Summary header.
        <header class="release-head accent-surface" style=format!("--accent:{status_rail}")>
            <div class="overline">"RELEASE TRUTH"</div>
            <div class="release-head__row">
                <h2 class="release-head__title">{name}</h2>
                {(!version.is_empty()).then(|| view! {
                    <span class="mono chip release-head__version">{version}</span>
                })}
            </div>
            <div class="chip-row release-head__meta">
                <span class="chip chip--state" style=format!("color:{status_rail}")>{status}</span>
                <span class="chip chip--state" style=format!("color:{posture_rail}")>{posture}</span>
                {(!target.is_empty()).then(|| view! {
                    <span class="mono chip release-head__target">{target}</span>
                })}
            </div>
            {doc.summary.clone().map(|s| view! { <p class="release-head__summary">{s}</p> })}
            // Required-scope completion bar.
            <div class="release-progress" title=format!("{progress}% of required scope complete")>
                <div class="release-progress__bar" style=format!("width:{progress}%")></div>
            </div>
            <span class="release-progress__label mono">{format!("{progress}% required complete")}</span>
        </header>

        // Release Path — milestone → item progression.
        <div class="overline release-panel__section">"RELEASE PATH"</div>
        <div class="release-path">
            {path.into_iter().map(milestone_step).collect_view()}
        </div>

        // Workstreams list with progress.
        {(!doc.workstreams.is_empty()).then(|| {
            let workstreams = doc.workstreams.clone();
            view! {
                <div class="overline release-panel__section">"WORKSTREAMS"</div>
                <div class="release-workstreams">
                    {workstreams.into_iter().map(|w| {
                        let rail = release_tone_color_var(release_tone(w.posture.as_deref()));
                        let prog = w.progress.unwrap_or(0).clamp(0, 100);
                        view! {
                            <div class="accent-surface release-ws" style=format!("--accent:{rail}")>
                                <div class="release-ws__head">
                                    <strong class="release-ws__name">{w.name}</strong>
                                    <span class="mono release-ws__progress">{format!("{prog}%")}</span>
                                </div>
                                <div class="chip-row">
                                    {w.status.map(|s| {
                                        let sr = release_tone_color_var(release_tone(Some(&s)));
                                        view! { <span class="chip chip--state" style=format!("color:{sr}")>{s}</span> }
                                    })}
                                    {w.posture.map(|p| view! {
                                        <span class="chip chip--state" style=format!("color:{rail}")>{p}</span>
                                    })}
                                    {w.owner.map(|o| view! { <span class="chip">{o}</span> })}
                                </div>
                                {w.summary.map(|s| view! { <p class="release-ws__summary">{s}</p> })}
                                <div class="release-progress">
                                    <div class="release-progress__bar" style=format!("width:{prog}%")></div>
                                </div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        })}
    }
}

/// One Release Path milestone step: marker + coarse line + sub-items.
fn milestone_step(m: MilestoneView) -> impl IntoView {
    let rail = release_tone_color_var(release_tone(Some(&m.status)));
    let blocked_by = (!m.blocked_by.is_empty()).then(|| {
        format!("Blocked by: {}", m.blocked_by.join(", "))
    });
    view! {
        <article class="release-path-step accent-surface" style=format!("--accent:{rail}")>
            <div class="release-path-marker mono">{m.path_number}</div>
            <div class="release-path-body">
                <div class="release-path-coarse">
                    <span class="chip chip--state release-path-state" style=format!("color:{rail}")>{m.line_state.clone()}</span>
                    <strong class="release-path-label">{m.label.clone()}</strong>
                    <span class="release-path-desc mono">
                        {format!("{} · {} · {} items", m.timing, m.completion_text, m.item_count)}
                    </span>
                    {blocked_by.map(|b| view! { <span class="release-path-blockers">{b}</span> })}
                </div>
                <div class="release-path-items">
                    {if m.items.is_empty() {
                        view! { <p class="release-panel__hint">"No linked release items."</p> }.into_view()
                    } else {
                        m.items.into_iter().map(path_item_line).collect_view().into_view()
                    }}
                </div>
            </div>
        </article>
    }
}

/// One Release Path item line: state chip + title + summary + meta.
fn path_item_line(item: PathItem) -> impl IntoView {
    // Tone keys off the item's status (blocked items already read "Blocked").
    let rail = release_tone_color_var(release_tone(item.status.as_deref()));
    // Clicking the line selects the item (drives the inspector detail). Read the
    // shared signal from context so the free helper stays a pure render fn.
    let sel = use_app_state().selected_release_item;
    let id = item.id.clone();
    let id_active = id.clone();
    let is_selected = move || sel.get().as_deref() == Some(id_active.as_str());
    let on_click = move |_| sel.set(Some(id.clone()));
    let kind = item.kind.clone().unwrap_or_default();
    let status = item.status.clone().unwrap_or_else(|| "planned".to_string());
    let priority = item.priority.clone();
    let owner = item.owner.clone();
    let meta = {
        let mut parts = vec![item.scope.clone(), item.workstream_name.clone(), status, kind];
        if let Some(p) = priority {
            parts.push(format!("{p} priority"));
        }
        if let Some(o) = owner {
            parts.push(o);
        }
        parts.retain(|p| !p.is_empty());
        parts.join(" · ")
    };
    let blocked_by = item.blocked_by.clone().map(|b| format!("Blocked by: {b}"));

    view! {
        <div
            class="release-path-line accent-surface"
            class=("release-path-line--selected", is_selected)
            style=format!("--accent:{rail}")
            on:click=on_click
        >
            <span class="chip chip--state release-path-state" style=format!("color:{rail}")>{item.line_state.clone()}</span>
            <div class="release-path-line__main">
                <strong>{item.title.clone()}</strong>
                {item.summary.clone().map(|s| view! { <span class="release-path-line__summary">{s}</span> })}
                <small class="release-path-line__meta mono">{meta}</small>
            </div>
            {blocked_by.map(|b| view! { <span class="release-path-blockers">{b}</span> })}
        </div>
    }
}
