//! Rules EDITOR — the detail pane turned into an upsert/delete/reorder form.
//!
//! The list + category filter stay in [`super::rules_panel`]; this component
//! owns the right-hand pane. It posts to `POST /api/rules` via
//! [`crate::data::post_mutation`] and lets the live-reload SSE stream refresh
//! the list (the serve fs-watcher broadcasts a `valid` event on the write,
//! which fires `AppState::reload_data`). The panel-local selection signal
//! (`selected_rule`) survives that reload, so the edited rule stays selected.
//!
//! Editing posture (faithful to the JS domain in
//! `src/domain/architecture-model/rules.mjs`):
//!   - `edit`-protected rules cannot be updated (Save disabled; the server also
//!     rejects with `ok:false`, which we surface inline as a fail-loud guard).
//!   - `delete`-protected rules cannot be deleted (Delete disabled).
//!   - reordering is forbidden when a rule is edit- OR delete-protected
//!     (`protectedFromReorder`), and only swaps within the same criticality.
//!   - a rejected/rolled-back write surfaces the server's message inline; the
//!     on-disk file is unchanged (the server rolls back), so the list is
//!     consistent.

use leptos::*;
use serde_json::{json, Value};

use crate::data::models::{Rule, RuleProtection};
use crate::data::post_mutation;
use crate::rule_order::criticality_color_var;
use crate::state::{use_app_state, AppState};

/// The criticality options for the select, in severity order. The empty value
/// is "unranked" (the rule carries no `criticality`).
pub const CRITICALITY_OPTIONS: &[&str] = &["critical", "high", "medium", "low"];

/// Whether a draft is reorderable: protected from reordering when EITHER
/// protection flag is set (port of JS `protectedFromReorder`).
pub fn is_reorder_protected(protection: &RuleProtection) -> bool {
    protection.edit || protection.delete
}

/// Build the `update` upsert payload: `{action:"update", rule:<full rule>}`.
/// The full rule is serialized so the server upserts the complete shape.
pub fn build_update_payload(rule: &Rule) -> Value {
    json!({ "action": "update", "rule": rule })
}

/// Build the `delete` payload: `{action:"delete", id}`.
pub fn build_delete_payload(id: &str) -> Value {
    json!({ "action": "delete", "id": id })
}

/// Build the `move` payload: `{action:"move", id, direction}`.
pub fn build_move_payload(id: &str, direction: &str) -> Value {
    json!({ "action": "move", "id": id, "direction": direction })
}

/// A live, editable copy of a rule's fields, backed by signals. Add mode seeds
/// every field blank (optionally pre-filling the category from the active
/// filter); Edit mode seeds from the selected rule.
#[derive(Clone, Copy)]
struct Draft {
    /// Whether this draft creates a NEW rule (id editable) vs edits an existing
    /// one (id fixed — it is the upsert key).
    is_new: RwSignal<bool>,
    id: RwSignal<String>,
    title: RwSignal<String>,
    summary: RwSignal<String>,
    category: RwSignal<String>,
    /// Empty string == unranked.
    criticality: RwSignal<String>,
    /// Stored as text; parsed to an optional i64 on save (empty == None).
    order: RwSignal<String>,
    source: RwSignal<String>,
    protect_edit: RwSignal<bool>,
    protect_delete: RwSignal<bool>,
}

impl Draft {
    fn blank(category: String) -> Self {
        Self {
            is_new: create_rw_signal(true),
            id: create_rw_signal(String::new()),
            title: create_rw_signal(String::new()),
            summary: create_rw_signal(String::new()),
            category: create_rw_signal(category),
            criticality: create_rw_signal("medium".to_string()),
            order: create_rw_signal(String::new()),
            source: create_rw_signal("maintainer".to_string()),
            protect_edit: create_rw_signal(false),
            protect_delete: create_rw_signal(false),
        }
    }

    fn from_rule(rule: &Rule) -> Self {
        Self {
            is_new: create_rw_signal(false),
            id: create_rw_signal(rule.id.clone()),
            title: create_rw_signal(rule.title.clone()),
            summary: create_rw_signal(rule.summary.clone().unwrap_or_default()),
            category: create_rw_signal(rule.category.clone().unwrap_or_default()),
            criticality: create_rw_signal(rule.criticality.clone().unwrap_or_default()),
            order: create_rw_signal(rule.order.map(|o| o.to_string()).unwrap_or_default()),
            source: create_rw_signal(rule.source.clone().unwrap_or_default()),
            protect_edit: create_rw_signal(rule.protection.edit),
            protect_delete: create_rw_signal(rule.protection.delete),
        }
    }

    /// Materialize the draft into a `Rule` for the update payload. Empty text
    /// fields collapse to `None` (so we don't write empty strings to the file).
    fn build_rule(&self) -> Rule {
        let opt = |s: String| if s.trim().is_empty() { None } else { Some(s) };
        Rule {
            id: self.id.get_untracked().trim().to_string(),
            title: self.title.get_untracked(),
            summary: opt(self.summary.get_untracked()),
            category: opt(self.category.get_untracked()),
            criticality: opt(self.criticality.get_untracked()),
            order: self.order.get_untracked().trim().parse::<i64>().ok(),
            source: opt(self.source.get_untracked()),
            protection: RuleProtection {
                edit: self.protect_edit.get_untracked(),
                delete: self.protect_delete.get_untracked(),
            },
        }
    }
}

/// The active editor target: editing the selected rule, or adding a new one.
#[derive(Clone)]
enum EditorTarget {
    Edit(Rule),
    /// Add a new rule, pre-filling the category (empty when none is active).
    Add(String),
}

/// The Rules editor pane. `selected_rule` is the panel-local selection signal
/// (shared with the list); `active_category` is the current filter (used to
/// pre-fill a new rule's category). `adding` toggles Add mode.
#[component]
pub fn RulesEditor(
    selected_rule: RwSignal<Option<String>>,
    active_category: RwSignal<String>,
    adding: RwSignal<bool>,
    all_categories_sentinel: &'static str,
) -> impl IntoView {
    let state = use_app_state();
    // Inline status: an error (server message) or a transient pending flag.
    let error = create_rw_signal::<Option<String>>(None);
    let pending = create_rw_signal(false);

    view! {
        {move || {
            // Recompute the target whenever Add mode or the selection changes.
            let target = if adding.get() {
                // Pre-fill a new rule's category from the active filter (unless
                // the "All" sentinel is active, which is not a real category).
                let cat = active_category.get();
                let prefill = if cat == all_categories_sentinel { String::new() } else { cat };
                Some(EditorTarget::Add(prefill))
            } else {
                let data = state.data.get();
                selected_rule
                    .get()
                    .and_then(|id| data.rules.iter().find(|r| r.id == id).cloned())
                    .map(EditorTarget::Edit)
            };
            // Reset transient status when the target changes.
            error.set(None);
            pending.set(false);

            match target {
                Some(target) => editor_form(
                    state, target, selected_rule, adding, error, pending,
                ).into_view(),
                None => view! {
                    <p class="rules-panel__hint">"Select a rule to edit, or add a new one."</p>
                }.into_view(),
            }
        }}
    }
}

/// The form for a single target (edit or add). Built fresh each time the target
/// changes so the draft signals re-seed from the current rule.
fn editor_form(
    state: AppState,
    target: EditorTarget,
    selected_rule: RwSignal<Option<String>>,
    adding: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
) -> impl IntoView {
    let (draft, edit_protected, delete_protected) = match &target {
        EditorTarget::Edit(rule) => (
            Draft::from_rule(rule),
            rule.protection.edit,
            rule.protection.delete,
        ),
        EditorTarget::Add(category) => (Draft::blank(category.clone()), false, false),
    };
    let is_new = draft.is_new.get_untracked();
    let reorder_protected = edit_protected || delete_protected;

    // Header rail tracks the chosen criticality (its own --sev-* scale).
    let rail = move || criticality_color_var(opt_str(draft.criticality.get()).as_deref());

    // ── Save (update upsert) ──────────────────────────────────────────────
    let on_save = move |_| {
        if edit_protected {
            error.set(Some("This rule is edit protected and cannot be changed.".to_string()));
            return;
        }
        let rule = draft.build_rule();
        if rule.id.is_empty() {
            error.set(Some("A rule id is required.".to_string()));
            return;
        }
        if rule.title.trim().is_empty() {
            error.set(Some("A rule title is required.".to_string()));
            return;
        }
        let payload = build_update_payload(&rule);
        post_and_select(state, payload, Some(rule.id.clone()), selected_rule, adding, error, pending);
    };

    // ── Delete ────────────────────────────────────────────────────────────
    let delete_id = draft.id.get_untracked();
    let on_delete = move |_| {
        if delete_protected {
            error.set(Some("This rule is delete protected and cannot be removed.".to_string()));
            return;
        }
        let payload = build_delete_payload(&delete_id);
        // Deleting clears the selection (the rule is gone after reload).
        post_and_select(state, payload, None, selected_rule, adding, error, pending);
    };

    // ── Move up / down (within the criticality tier) ──────────────────────
    let move_id = draft.id.get_untracked();
    let move_handler = move |direction: &'static str| {
        let id = move_id.clone();
        move |_| {
            if reorder_protected {
                error.set(Some("Protected rules cannot be reordered.".to_string()));
                return;
            }
            let payload = build_move_payload(&id, direction);
            post_and_select(
                state, payload, Some(id.clone()), selected_rule, adding, error, pending,
            );
        }
    };
    let on_move_up = move_handler("up");
    let on_move_down = move_handler("down");

    let on_cancel = move |_| {
        adding.set(false);
        error.set(None);
    };

    let token_missing = move || state.mutation_token.get().is_none();

    view! {
        <div class="accent-surface rule-detail rule-editor" style=move || format!("--accent:{}", rail())>
            <div class="overline">{move || if is_new { "ADD RULE" } else { "EDIT RULE" }}</div>

            <label class="rule-editor__field">
                <span class="rule-editor__label">"id"</span>
                <input class="rule-editor__input mono" type="text"
                    prop:value=move || draft.id.get()
                    prop:disabled=!is_new
                    on:input=move |e| draft.id.set(event_target_value(&e))
                />
            </label>

            <label class="rule-editor__field">
                <span class="rule-editor__label">"title"</span>
                <input class="rule-editor__input" type="text"
                    prop:value=move || draft.title.get()
                    prop:disabled=edit_protected
                    on:input=move |e| draft.title.set(event_target_value(&e))
                />
            </label>

            <label class="rule-editor__field">
                <span class="rule-editor__label">"summary"</span>
                <textarea class="rule-editor__input rule-editor__textarea"
                    prop:value=move || draft.summary.get()
                    prop:disabled=edit_protected
                    on:input=move |e| draft.summary.set(event_target_value(&e))
                />
            </label>

            <div class="rule-editor__row">
                <label class="rule-editor__field">
                    <span class="rule-editor__label">"criticality"</span>
                    <select class="rule-editor__input"
                        prop:disabled=edit_protected
                        on:change=move |e| draft.criticality.set(event_target_value(&e))
                    >
                        <option value="" selected=move || draft.criticality.get().is_empty()>"unranked"</option>
                        {CRITICALITY_OPTIONS.iter().map(|opt| {
                            let opt = opt.to_string();
                            let value = opt.clone();
                            view! {
                                <option value=value.clone()
                                    selected=move || draft.criticality.get() == value
                                >{opt}</option>
                            }
                        }).collect_view()}
                    </select>
                </label>

                <label class="rule-editor__field rule-editor__field--narrow">
                    <span class="rule-editor__label">"order"</span>
                    <input class="rule-editor__input mono" type="number"
                        prop:value=move || draft.order.get()
                        prop:disabled=edit_protected
                        on:input=move |e| draft.order.set(event_target_value(&e))
                    />
                </label>
            </div>

            <div class="rule-editor__row">
                <label class="rule-editor__field">
                    <span class="rule-editor__label">"category"</span>
                    <input class="rule-editor__input" type="text"
                        prop:value=move || draft.category.get()
                        prop:disabled=edit_protected
                        on:input=move |e| draft.category.set(event_target_value(&e))
                    />
                </label>
                <label class="rule-editor__field">
                    <span class="rule-editor__label">"source"</span>
                    <input class="rule-editor__input mono" type="text"
                        prop:value=move || draft.source.get()
                        prop:disabled=edit_protected
                        on:input=move |e| draft.source.set(event_target_value(&e))
                    />
                </label>
            </div>

            <div class="rule-editor__protection">
                <span class="rule-editor__label">"protection"</span>
                <label class="rule-editor__check">
                    <input type="checkbox"
                        prop:checked=move || draft.protect_edit.get()
                        prop:disabled=edit_protected
                        on:change=move |e| draft.protect_edit.set(event_target_checked(&e))
                    />
                    <span>"edit"</span>
                </label>
                <label class="rule-editor__check">
                    <input type="checkbox"
                        prop:checked=move || draft.protect_delete.get()
                        prop:disabled=edit_protected
                        on:change=move |e| draft.protect_delete.set(event_target_checked(&e))
                    />
                    <span>"delete"</span>
                </label>
            </div>

            // Inline status: server-message error or token-missing notice.
            {move || error.get().map(|msg| view! {
                <p class="rule-editor__error">{msg}</p>
            })}
            {move || token_missing().then(|| view! {
                <p class="rule-editor__error">"Editing is not authorized in this session (no mutation token)."</p>
            })}

            <div class="rule-editor__actions">
                <button class="rule-editor__btn rule-editor__btn--primary"
                    prop:disabled=move || pending.get() || edit_protected || token_missing()
                    on:click=on_save
                >{move || if pending.get() { "Saving…" } else { "Save" }}</button>

                {(!is_new).then(|| view! {
                    <button class="rule-editor__btn"
                        prop:disabled=move || pending.get() || delete_protected || token_missing()
                        on:click=on_delete
                    >"Delete"</button>
                    <button class="rule-editor__btn"
                        prop:disabled=move || pending.get() || reorder_protected || token_missing()
                        on:click=on_move_up
                    >"Move up"</button>
                    <button class="rule-editor__btn"
                        prop:disabled=move || pending.get() || reorder_protected || token_missing()
                        on:click=on_move_down
                    >"Move down"</button>
                }.into_view())}

                {is_new.then(|| view! {
                    <button class="rule-editor__btn" on:click=on_cancel>"Cancel"</button>
                }.into_view())}
            </div>
        </div>
    }
}

/// POST the payload, then on success preserve/clear the selection and leave Add
/// mode; the SSE `valid` event refreshes the list. On failure surface the
/// server's message inline (the file is unchanged — server rolled back).
fn post_and_select(
    state: AppState,
    payload: Value,
    select_id: Option<String>,
    selected_rule: RwSignal<Option<String>>,
    adding: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
) {
    let token = state.mutation_token.get_untracked();
    error.set(None);
    pending.set(true);
    spawn_local(async move {
        let result = post_mutation(token.as_deref(), "/api/rules", &payload).await;
        pending.set(false);
        match result {
            Ok(_) => {
                // Single source of truth = the SSE-reloaded data. We only steer
                // the panel-local selection (which survives reload).
                selected_rule.set(select_id);
                adding.set(false);
            }
            Err(err) => error.set(Some(err.message)),
        }
    });
}

/// `""` → `None`, else `Some(s)`.
fn opt_str(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rule() -> Rule {
        Rule {
            id: "r1".into(),
            title: "Title".into(),
            summary: Some("Sum".into()),
            category: Some("Architecture".into()),
            criticality: Some("high".into()),
            order: Some(3),
            source: Some("maintainer".into()),
            protection: RuleProtection { edit: false, delete: false },
        }
    }

    #[test]
    fn update_payload_carries_action_and_full_rule() {
        let payload = build_update_payload(&sample_rule());
        assert_eq!(payload["action"], "update");
        assert_eq!(payload["rule"]["id"], "r1");
        assert_eq!(payload["rule"]["criticality"], "high");
        assert_eq!(payload["rule"]["order"], 3);
        // protection is always present (object), even when both false.
        assert_eq!(payload["rule"]["protection"]["edit"], false);
        assert_eq!(payload["rule"]["protection"]["delete"], false);
    }

    #[test]
    fn update_payload_omits_empty_optionals() {
        let mut rule = sample_rule();
        rule.summary = None;
        rule.category = None;
        rule.criticality = None;
        rule.order = None;
        rule.source = None;
        let payload = build_update_payload(&rule);
        let obj = payload["rule"].as_object().unwrap();
        assert!(!obj.contains_key("summary"));
        assert!(!obj.contains_key("category"));
        assert!(!obj.contains_key("criticality"));
        assert!(!obj.contains_key("order"));
        assert!(!obj.contains_key("source"));
        // id/title/protection are always present.
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("title"));
        assert!(obj.contains_key("protection"));
    }

    #[test]
    fn delete_and_move_payloads_match_contract() {
        assert_eq!(build_delete_payload("r1"), json!({ "action": "delete", "id": "r1" }));
        assert_eq!(
            build_move_payload("r1", "up"),
            json!({ "action": "move", "id": "r1", "direction": "up" })
        );
        assert_eq!(
            build_move_payload("r1", "down"),
            json!({ "action": "move", "id": "r1", "direction": "down" })
        );
    }

    #[test]
    fn reorder_protected_when_either_flag_set() {
        assert!(!is_reorder_protected(&RuleProtection { edit: false, delete: false }));
        assert!(is_reorder_protected(&RuleProtection { edit: true, delete: false }));
        assert!(is_reorder_protected(&RuleProtection { edit: false, delete: true }));
        assert!(is_reorder_protected(&RuleProtection { edit: true, delete: true }));
    }

    #[test]
    fn criticality_options_are_severity_ordered() {
        assert_eq!(CRITICALITY_OPTIONS, &["critical", "high", "medium", "low"]);
    }
}
