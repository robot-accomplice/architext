//! Release Planning EDITOR — the editable counterpart to the read-only Release
//! Truth display.
//!
//! Faithful port of `viewer/src/presentation/ReleasePlanning.tsx`. It posts to
//! `POST /api/release-plans` via [`crate::data::post_mutation`] with three
//! actions:
//!   - **preview** (`dryRun`) — no write; the server returns the would-be plan
//!     plus `changes` + `validation`, which we render inline (no SSE fires, so
//!     the preview is the only feedback).
//!   - **save-draft** — writes the release detail + index (NOT roadmap).
//!   - **approve** — writes detail + index + roadmap.
//!
//! save-draft/approve land in the watched data dir, so the serve fs-watcher
//! broadcasts a reload (`AppState::reload_data`) and the read-only display
//! refreshes automatically; we only clear the local editor status.
//!
//! Pure logic (next-minor version, candidate filter, payload assembly, editable
//! seeding) lives in [`crate::release_planning_model`] and is native-tested.
//!
//! DESIGN.md fidelity: this is a STATE/editing surface, so selection and the
//! primary/approve actions use the dedicated `--accent` treatment (never a
//! `--c4-*` role hue). Cards are `.accent-surface`, section labels `.overline`,
//! the version/scope meta is `mono`.

use std::collections::BTreeMap;

use leptos::*;

use crate::data::models::RoadmapItem;
use crate::data::post_mutation;
use crate::release_planning_model::{
    editable_release_scope, next_minor_version_from_releases, planning_candidate_items,
    release_plan_action_disabled, release_plan_proposal_payload, scope_label, sort_roadmap_items,
    AdHocPlanningItem, DEFAULT_SCOPE, KIND_OPTIONS, PRIORITY_OPTIONS, SCOPE_VALUES,
};
use crate::release_truth::ReleaseDoc;
use crate::state::use_app_state;

/// The editor's mutable form state, all signal-backed.
#[derive(Clone, Copy)]
struct PlanDraft {
    version: RwSignal<String>,
    theme: RwSignal<String>,
    /// `id -> selected` for roadmap candidate items.
    selected: RwSignal<std::collections::BTreeSet<String>>,
    /// `id -> scope value` overrides for roadmap items.
    item_scopes: RwSignal<BTreeMap<String, String>>,
    ad_hoc: RwSignal<Vec<AdHocPlanningItem>>,
    // New ad-hoc item form fields.
    ad_hoc_open: RwSignal<bool>,
    new_title: RwSignal<String>,
    new_summary: RwSignal<String>,
    new_kind: RwSignal<String>,
    new_priority: RwSignal<String>,
    new_section: RwSignal<String>,
    new_scope: RwSignal<String>,
}

impl PlanDraft {
    /// Seed from an existing unreleased detail when editing, else a fresh plan.
    fn seed(versions: &[Option<String>], detail: Option<&ReleaseDoc>) -> Self {
        let (version, selected, item_scopes, ad_hoc) = match detail {
            Some(doc) => {
                let scope = editable_release_scope(doc);
                (
                    doc.version.clone().unwrap_or_default(),
                    scope.selected_roadmap_ids.into_iter().collect(),
                    scope.item_scopes,
                    scope.ad_hoc_items,
                )
            }
            None => (
                next_minor_version_from_releases(versions),
                std::collections::BTreeSet::new(),
                BTreeMap::new(),
                Vec::new(),
            ),
        };
        Self {
            version: create_rw_signal(version),
            theme: create_rw_signal(String::new()),
            selected: create_rw_signal(selected),
            item_scopes: create_rw_signal(item_scopes),
            ad_hoc: create_rw_signal(ad_hoc),
            ad_hoc_open: create_rw_signal(false),
            new_title: create_rw_signal(String::new()),
            new_summary: create_rw_signal(String::new()),
            new_kind: create_rw_signal("feature".to_string()),
            new_priority: create_rw_signal("medium".to_string()),
            new_section: create_rw_signal("Release Planning".to_string()),
            new_scope: create_rw_signal(DEFAULT_SCOPE.to_string()),
        }
    }
}

/// Parsed preview `changes` + `validation` for inline display.
struct PreviewView {
    release_file: String,
    index_line: String,
    roadmap_line: String,
    validation: String,
}

fn parse_preview(value: &serde_json::Value) -> Option<PreviewView> {
    let changes = value.get("changes")?;
    if changes.is_null() {
        return None;
    }
    let rf = &changes["releaseFile"];
    let ri = &changes["releaseIndex"];
    let rm = &changes["roadmap"];
    let release_file = format!(
        "{} {}",
        rf["action"].as_str().unwrap_or(""),
        rf["file"].as_str().unwrap_or("")
    );
    let index_line = format!(
        "{}; current release becomes {}",
        ri["action"].as_str().unwrap_or(""),
        ri["currentReleaseId"].as_str().unwrap_or("")
    );
    let roadmap_line = format!(
        "Roadmap: {} added · {} retargeted · {} unchanged",
        rm["add"].as_i64().unwrap_or(0),
        rm["retarget"].as_i64().unwrap_or(0),
        rm["unchanged"].as_i64().unwrap_or(0)
    );
    let validation = value["validation"]["output"]
        .as_str()
        .unwrap_or("Preview passed.")
        .to_string();
    Some(PreviewView { release_file, index_line, roadmap_line, validation })
}

/// The Release Planning editor. `active_detail` is the unreleased plan being
/// edited (the JS `editableRelease`); `None` means planning a fresh next
/// release. `on_done` is called after a successful save/approve so the host can
/// leave planning mode.
#[component]
pub fn ReleasePlanningEditor(
    /// All release summary versions, for `nextMinorVersionFromReleases`.
    versions: Vec<Option<String>>,
    /// The id of the release being edited (used for the candidate filter), or
    /// `None` for a fresh plan.
    active_release_id: Option<String>,
    /// The existing detail to seed from (an unreleased plan being edited).
    active_detail: Option<ReleaseDoc>,
    on_done: Callback<()>,
) -> impl IntoView {
    let state = use_app_state();
    let editing_existing = active_detail.is_some();
    let draft = PlanDraft::seed(&versions, active_detail.as_ref());

    let error = create_rw_signal::<Option<String>>(None);
    let message = create_rw_signal::<Option<String>>(None);
    let preview = create_rw_signal::<Option<serde_json::Value>>(None);
    let pending = create_rw_signal(false);

    let token_missing = move || state.mutation_token.get().is_none();

    // ── Selected count + disabled gate (reactive, Copy-able signals) ────────
    let active_id = active_release_id.clone();
    let count_active_id = active_id.clone();
    let selected_count = Signal::derive(move || {
        let data = state.data.get();
        let cands: std::collections::BTreeSet<String> =
            planning_candidate_items(&data.roadmap, count_active_id.as_deref())
                .into_iter()
                .map(|i| i.id)
                .collect();
        let sel = draft.selected.get();
        let roadmap_selected = sel.iter().filter(|id| cands.contains(*id)).count();
        roadmap_selected + draft.ad_hoc.get().len()
    });
    let disabled = Signal::derive(move || {
        release_plan_action_disabled(pending.get(), &draft.version.get(), selected_count.get())
            || token_missing()
    });

    // ── Submit ──────────────────────────────────────────────────────────────
    let active_id_submit = active_id.clone();
    let submit = move |action: &'static str| {
        let active_id = active_id_submit.clone();
        move |_| {
            let dry_run = action == "preview";
            // Assemble the payload from the current draft + live candidate set.
            let data = state.data.get_untracked();
            let cands: std::collections::BTreeSet<String> =
                planning_candidate_items(&data.roadmap, active_id.as_deref())
                    .into_iter()
                    .map(|i| i.id)
                    .collect();
            let selected_roadmap_ids: Vec<String> = draft
                .selected
                .get_untracked()
                .into_iter()
                .filter(|id| cands.contains(id))
                .collect();
            let payload = release_plan_proposal_payload(
                dry_run,
                action,
                &draft.version.get_untracked(),
                &draft.theme.get_untracked(),
                &selected_roadmap_ids,
                &draft.item_scopes.get_untracked(),
                &draft.ad_hoc.get_untracked(),
            );
            let token = state.mutation_token.get_untracked();
            error.set(None);
            message.set(None);
            pending.set(true);
            spawn_local(async move {
                let result = post_mutation(token.as_deref(), "/api/release-plans", &payload).await;
                pending.set(false);
                match result {
                    Ok(value) => {
                        let name = value["release"]["name"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_default();
                        if dry_run {
                            preview.set(Some(value));
                            message.set(Some(format!(
                                "Preview ready for {}.",
                                if name.is_empty() { "the next release".to_string() } else { name }
                            )));
                        } else {
                            preview.set(None);
                            message.set(Some(if action == "save-draft" {
                                format!("Saved draft {name}.")
                            } else {
                                format!("Created {name}.")
                            }));
                            // SSE reload refreshes the read-only display; leave planning.
                            on_done.call(());
                        }
                    }
                    Err(err) => error.set(Some(err.message)),
                }
            });
        }
    };
    let on_preview = submit("preview");
    let on_save_draft = submit("save-draft");
    let on_approve = submit("approve");

    // ── Add ad-hoc item ──────────────────────────────────────────────────────
    let add_ad_hoc = Callback::new(move |_: ()| {
        let title = draft.new_title.get_untracked().trim().to_string();
        if title.is_empty() {
            return;
        }
        let summary = {
            let s = draft.new_summary.get_untracked().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let section = {
            let s = draft.new_section.get_untracked().trim().to_string();
            if s.is_empty() { "Ad hoc".to_string() } else { s }
        };
        let id = format!("ad-hoc-{}", next_ad_hoc_seq());
        draft.ad_hoc.update(|items| {
            items.push(AdHocPlanningItem {
                id,
                persisted: false,
                title,
                summary,
                kind: draft.new_kind.get_untracked(),
                priority: draft.new_priority.get_untracked(),
                section,
                scope: draft.new_scope.get_untracked(),
            });
        });
        draft.new_title.set(String::new());
        draft.new_summary.set(String::new());
        draft.new_scope.set(DEFAULT_SCOPE.to_string());
        draft.ad_hoc_open.set(false);
        preview.set(None);
    });

    let cancel_editing = move |_| on_done.call(());

    view! {
        <section class="accent-surface release-planning" >
            <div class="release-planning__head">
                <div>
                    <div class="overline">"RELEASE PLANNING"</div>
                    <h3 class="release-planning__title">
                        {if editing_existing { "Edit unreleased plan" } else { "Plan next release" }}
                    </h3>
                    <p class="release-planning__sub">
                        {if editing_existing {
                            "Update this unreleased plan, or approve it when the scope is ready."
                        } else {
                            "Select roadmap work, add ad hoc items, then save a draft or approve one next-release plan."
                        }}
                    </p>
                </div>
                <div class="release-planning__fields">
                    <label class="release-planning__field">
                        <span class="release-planning__label">"version"</span>
                        <input class="release-planning__input mono" type="text"
                            prop:value=move || draft.version.get()
                            on:input=move |e| { draft.version.set(event_target_value(&e)); preview.set(None); }
                        />
                    </label>
                    <label class="release-planning__field">
                        <span class="release-planning__label">"theme"</span>
                        <input class="release-planning__input" type="text" placeholder="Optional"
                            prop:value=move || draft.theme.get()
                            on:input=move |e| { draft.theme.set(event_target_value(&e)); preview.set(None); }
                        />
                    </label>
                </div>
            </div>

            // ── Candidate roadmap items + selected ad-hoc rows ───────────────
            <div class="release-planning__items">
                {move || {
                    let data = state.data.get();
                    let cands = sort_roadmap_items(&planning_candidate_items(
                        &data.roadmap, active_id.as_deref(),
                    ));
                    let sel = draft.selected.get();
                    let scopes = draft.item_scopes.get();
                    let roadmap_rows = cands.into_iter().map(|item| {
                        let id = item.id.clone();
                        let checked = sel.contains(&id);
                        let scope = scopes.get(&id).cloned().unwrap_or_else(|| DEFAULT_SCOPE.to_string());
                        roadmap_row(draft, preview, item, checked, scope)
                    }).collect_view();
                    let ad_hoc_rows = draft.ad_hoc.get().into_iter().map(|item| {
                        ad_hoc_row(draft, preview, item)
                    }).collect_view();
                    view! { {roadmap_rows} {ad_hoc_rows} }.into_view()
                }}
            </div>

            // ── New ad-hoc item drawer ───────────────────────────────────────
            <div class="release-planning__ad-hoc-footer">
                <button class="release-planning__btn" type="button"
                    on:click=move |_| draft.ad_hoc_open.update(|v| *v = !*v)
                >
                    {move || if draft.ad_hoc_open.get() { "Close new item" } else { "Add new item" }}
                </button>
                {move || draft.ad_hoc_open.get().then(|| ad_hoc_form(draft, add_ad_hoc).into_view())}
            </div>

            // ── Preview ──────────────────────────────────────────────────────
            {move || preview.get().as_ref().and_then(parse_preview).map(|p| view! {
                <div class="release-planning__preview">
                    <div class="overline">"PREVIEW"</div>
                    <span class="mono">{p.release_file}</span>
                    <span class="mono">{p.index_line}</span>
                    <span class="mono">{p.roadmap_line}</span>
                    <span>{p.validation}</span>
                </div>
            })}

            // ── Inline status ────────────────────────────────────────────────
            {move || error.get().map(|msg| view! {
                <p class="release-planning__error">{msg}</p>
            })}
            {move || token_missing().then(|| view! {
                <p class="release-planning__error">
                    "Editing is not authorized in this session (no mutation token)."
                </p>
            })}

            // ── Actions ──────────────────────────────────────────────────────
            <div class="release-planning__actions">
                <span class="mono release-planning__count">
                    {move || format!("{} selected items", selected_count.get())}
                </span>
                {move || message.get().map(|m| view! {
                    <span class="release-planning__message">{m}</span>
                })}
                <span class="release-planning__spacer"></span>
                <button class="release-planning__btn" type="button"
                    prop:disabled=disabled
                    on:click=on_preview
                >{move || if pending.get() { "Working…" } else { "Preview changes" }}</button>
                <button class="release-planning__btn release-planning__btn--primary" type="button"
                    prop:disabled=disabled
                    on:click=on_save_draft
                >"Save draft"</button>
                <button class="release-planning__btn release-planning__btn--approve" type="button"
                    prop:disabled=disabled
                    on:click=on_approve
                >"Approve release plan"</button>
                {editing_existing.then(|| view! {
                    <button class="release-planning__btn" type="button" on:click=cancel_editing>
                        "Cancel"
                    </button>
                })}
            </div>
        </section>
    }
}

/// A scope `<select>` bound to a setter. Shared by roadmap + ad-hoc rows.
fn scope_select(current: String, on_change: impl Fn(String) + 'static) -> impl IntoView {
    view! {
        <select class="release-planning__input release-planning__scope"
            on:change=move |e| on_change(event_target_value(&e))
        >
            {SCOPE_VALUES.iter().map(|value| {
                let value = value.to_string();
                let selected = value == current;
                view! {
                    <option value=value.clone() selected=selected>{scope_label(&value)}</option>
                }
            }).collect_view()}
        </select>
    }
}

/// One roadmap candidate row: checkbox + meta + (when selected) a scope select.
fn roadmap_row(
    draft: PlanDraft,
    preview: RwSignal<Option<serde_json::Value>>,
    item: RoadmapItem,
    checked: bool,
    scope: String,
) -> View {
    let id = item.id.clone();
    let toggle_id = id.clone();
    let on_toggle = move |_| {
        preview.set(None);
        draft.selected.update(|set| {
            if !set.remove(&toggle_id) {
                set.insert(toggle_id.clone());
            }
        });
    };
    let scope_id = id.clone();
    let on_scope = move |next: String| {
        preview.set(None);
        draft.item_scopes.update(|m| {
            m.insert(scope_id.clone(), next);
        });
    };
    let meta = format!(
        "{} · {} · {} · {} priority · roadmap",
        scope_label(&scope),
        item.section.clone().unwrap_or_else(|| "—".to_string()),
        item.kind.clone().unwrap_or_else(|| "feature".to_string()),
        item.priority.clone().unwrap_or_else(|| "medium".to_string()),
    );
    let summary = item.summary.clone();
    view! {
        <label class="accent-surface release-planning__option" class:is-active=checked>
            <input type="checkbox" prop:checked=checked on:change=on_toggle/>
            <span class="release-planning__option-main">
                <strong>{item.title.clone()}</strong>
                <small class="mono">{meta}</small>
                {summary.map(|s| view! { <em class="release-planning__option-summary">{s}</em> })}
            </span>
            {checked.then(|| scope_select(scope.clone(), on_scope).into_view())}
        </label>
    }
    .into_view()
}

/// One ad-hoc row (always selected). Unchecking removes it.
fn ad_hoc_row(
    draft: PlanDraft,
    preview: RwSignal<Option<serde_json::Value>>,
    item: AdHocPlanningItem,
) -> View {
    let id = item.id.clone();
    let remove_id = id.clone();
    let on_toggle = move |_| {
        preview.set(None);
        draft.ad_hoc.update(|items| items.retain(|i| i.id != remove_id));
    };
    let scope_id = id.clone();
    let on_scope = move |next: String| {
        preview.set(None);
        let scope_id = scope_id.clone();
        draft.ad_hoc.update(|items| {
            if let Some(found) = items.iter_mut().find(|i| i.id == scope_id) {
                found.scope = next;
            }
        });
    };
    let meta = format!(
        "{} · {} · {} · {} priority · ad-hoc",
        scope_label(&item.scope),
        item.section,
        item.kind,
        item.priority,
    );
    let summary = item.summary.clone();
    let scope = item.scope.clone();
    view! {
        <label class="accent-surface release-planning__option is-active">
            <input type="checkbox" prop:checked=true on:change=on_toggle/>
            <span class="release-planning__option-main">
                <strong>{item.title.clone()}</strong>
                <small class="mono">{meta}</small>
                {summary.map(|s| view! { <em class="release-planning__option-summary">{s}</em> })}
            </span>
            {scope_select(scope, on_scope).into_view()}
        </label>
    }
    .into_view()
}

/// The "new release item" form drawer.
fn ad_hoc_form(draft: PlanDraft, add_ad_hoc: Callback<()>) -> impl IntoView {
    let close = move |_| draft.ad_hoc_open.set(false);
    let title_empty = move || draft.new_title.get().trim().is_empty();
    let on_add = move |_| add_ad_hoc.call(());
    view! {
        <div class="release-planning__ad-hoc">
            <strong>"New release item"</strong>
            <input class="release-planning__input" type="text" placeholder="Title"
                prop:value=move || draft.new_title.get()
                on:input=move |e| draft.new_title.set(event_target_value(&e))
            />
            <input class="release-planning__input" type="text" placeholder="Summary (optional)"
                prop:value=move || draft.new_summary.get()
                on:input=move |e| draft.new_summary.set(event_target_value(&e))
            />
            <div class="release-planning__inline-fields">
                <select class="release-planning__input"
                    on:change=move |e| draft.new_kind.set(event_target_value(&e))
                >
                    {KIND_OPTIONS.iter().map(|(value, label)| {
                        let value = value.to_string();
                        let is_default = value == "feature";
                        view! { <option value=value.clone() selected=is_default>{*label}</option> }
                    }).collect_view()}
                </select>
                <select class="release-planning__input"
                    on:change=move |e| draft.new_priority.set(event_target_value(&e))
                >
                    {PRIORITY_OPTIONS.iter().map(|value| {
                        let value = value.to_string();
                        let is_default = value == "medium";
                        let label = format!("{}{}", value[..1].to_uppercase(), &value[1..]);
                        view! { <option value=value.clone() selected=is_default>{label}</option> }
                    }).collect_view()}
                </select>
                <select class="release-planning__input"
                    on:change=move |e| draft.new_scope.set(event_target_value(&e))
                >
                    {SCOPE_VALUES.iter().map(|value| {
                        let value = value.to_string();
                        let is_default = value == DEFAULT_SCOPE;
                        view! { <option value=value.clone() selected=is_default>{scope_label(&value)}</option> }
                    }).collect_view()}
                </select>
                <input class="release-planning__input" type="text" placeholder="Section"
                    prop:value=move || draft.new_section.get()
                    on:input=move |e| draft.new_section.set(event_target_value(&e))
                />
            </div>
            <div class="release-planning__form-actions">
                <button class="release-planning__btn release-planning__btn--primary" type="button"
                    prop:disabled=title_empty
                    on:click=on_add
                >"Add and select"</button>
                <button class="release-planning__btn" type="button" on:click=close>"Cancel"</button>
            </div>
        </div>
    }
}

/// A process-unique sequence for a fresh ad-hoc id. The JS uses `Date.now()`
/// for uniqueness within the editing session; a monotonic counter is equivalent
/// (the id is transient — the server mints the persisted id on approve) and
/// avoids pulling in the `js-sys` clock just for a temp key.
fn next_ad_hoc_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}
