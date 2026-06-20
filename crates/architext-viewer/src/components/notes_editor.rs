//! Notes EDITOR — the inspector's per-element annotation surface.
//!
//! Notes are user annotations attached to whatever element the inspector is
//! currently showing (a selected node → `{kind:"node"}`, otherwise the selected
//! view/flow). This component lists the notes for that target newest-first, and
//! lets the user add, edit, and delete them. It posts to `POST /api/notes` via
//! [`crate::data::post_mutation`] and relies on the live-reload SSE stream to
//! refresh the list: `notes.json` lives in the watched data dir, so a successful
//! write broadcasts a `valid` event which fires `AppState::reload_data`. We do
//! NOT mutate local state on success — the SSE reload is the single source of
//! truth (faithful to the Rules editor posture).
//!
//! Faithful to the JS reference (`viewer/src/presentation/NotesSection.tsx`):
//!   - list filtered to the target, sorted newest-first by `updatedAt`;
//!   - empty → "No notes yet.";
//!   - add/edit a category (note/mitigation/caveat/todo) + body; Save disabled
//!     while busy or the body is empty;
//!   - edit preserves `createdAt`, bumps `updatedAt`; delete removes by id.
//!
//! ## Client-minted id and timestamps
//! New notes need a schema-valid `id` (`^[a-z][a-z0-9-]*$`) and non-empty
//! `createdAt`/`updatedAt` (else the server's post-write validation rolls back).
//! The JS uses `note-${Date.now().toString(36)}`; we mint the same shape from
//! the browser clock via `js_sys::Date::now()` (ms since epoch) rendered in
//! base36 — lowercase alphanumeric, so `note-<base36>` matches the id pattern.
//! Timestamps use `js_sys::Date::new_0().to_iso_string()` (ISO 8601). These JS
//! calls only run in the wasm UI; the pure payload-assembly logic takes the id
//! and timestamps as parameters so it is unit-testable on native.

use leptos::*;
use serde_json::{json, Value};

use crate::data::models::{Note, NoteTarget};
use crate::data::post_mutation;
use crate::state::{use_app_state, AppState};

/// The note categories, in display order (value, label). Port of the JS
/// `CATEGORIES`.
pub const CATEGORIES: &[(&str, &str)] =
    &[("note", "Note"), ("mitigation", "Mitigation"), ("caveat", "Caveat"), ("todo", "To-do")];

/// Notes attached to one target, in stable newest-first order (by `updatedAt`,
/// descending). Pure port of the JS `notesForTarget`.
pub fn notes_for_target<'a>(notes: &'a [Note], kind: &str, id: &str) -> Vec<&'a Note> {
    let mut matched: Vec<&Note> =
        notes.iter().filter(|n| n.target.kind == kind && n.target.id == id).collect();
    // Newest first: descending lexicographic compare on `updatedAt` (ISO strings
    // sort chronologically), matching the JS `localeCompare` reverse.
    matched.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    matched
}

/// Assemble a `Note` for the upsert payload. `id`, `created_at`, `updated_at`
/// are supplied by the caller (the UI mints them from the browser clock) so this
/// stays a pure, testable transform. The body is trimmed; the category is taken
/// as-is. No `author` field is emitted (kept `None` → `skip_serializing_if`),
/// honoring the `additionalProperties:false` schema.
pub fn build_note(
    id: String,
    target_kind: String,
    target_id: String,
    category: String,
    body: String,
    created_at: String,
    updated_at: String,
) -> Note {
    Note {
        id,
        target: NoteTarget { kind: target_kind, id: target_id },
        category,
        body: body.trim().to_string(),
        author: None,
        created_at,
        updated_at,
    }
}

/// Build the `update` upsert payload: `{action:"update", note:<full note>}`.
pub fn build_update_payload(note: &Note) -> Value {
    json!({ "action": "update", "note": note })
}

/// Build the `delete` payload: `{action:"delete", id}`.
pub fn build_delete_payload(id: &str) -> Value {
    json!({ "action": "delete", "id": id })
}

/// Mint a client-side note id of the JS shape `note-<base36 ms>`. base36 of the
/// epoch-ms clock is lowercase alphanumeric, so the result matches the schema id
/// pattern `^[a-z][a-z0-9-]*$`. wasm-only (uses the browser clock).
fn mint_note_id() -> String {
    let ms = js_sys::Date::now() as u64;
    format!("note-{}", to_base36(ms))
}

/// Current time as an ISO 8601 string from the browser clock (wasm-only).
fn now_iso() -> String {
    js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default()
}

/// Render a `u64` in base36 (lowercase). Used for the note id suffix.
fn to_base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("base36 digits are ASCII")
}

/// The active editor draft: either editing an existing note (id fixed) or
/// adding a new one (id minted on save).
#[derive(Clone, Copy)]
struct Draft {
    /// The id of the note being edited, or `None` for a new note.
    edit_id: RwSignal<Option<String>>,
    category: RwSignal<String>,
    body: RwSignal<String>,
}

/// The Notes section for the inspector's current target (kind + id). Drops in
/// under the inspector card; the target is recomputed by the inspector each
/// render and passed in, so switching the selected node/view re-scopes the list.
///
/// `label` names whose notes these are ("View notes" / "Flow notes" /
/// "Node notes") so two stacked sections aren't ambiguous identical "NOTES"
/// blocks (UX review #6).
#[component]
pub fn NotesSection(label: String, target_kind: String, target_id: String) -> impl IntoView {
    let state = use_app_state();
    let error = create_rw_signal::<Option<String>>(None);
    let pending = create_rw_signal(false);
    // `None` = not editing; `Some` = the editor is open (new or existing).
    let draft = create_rw_signal::<Option<Draft>>(None);

    // The component params are consumed here into the reactive list closure;
    // the editor clones them per render so the closure stays `Fn`.
    let kind_for_list = target_kind;
    let id_for_list = target_id;

    let token_missing = move || state.mutation_token.get().is_none();

    let start_add = move |_| {
        error.set(None);
        draft.set(Some(Draft {
            edit_id: create_rw_signal(None),
            category: create_rw_signal("note".to_string()),
            body: create_rw_signal(String::new()),
        }));
    };

    view! {
        <div class="notes-section">
            <div class="notes-head">
                <div class="overline">{label.to_uppercase()}</div>
                {move || (draft.get().is_none() && !token_missing()).then(|| view! {
                    <button class="notes-add" on:click=start_add>"+ Add note"</button>
                })}
            </div>

            {move || token_missing().then(|| view! {
                <p class="notes-empty">"Editing is not authorized in this session (no mutation token)."</p>
            })}

            {move || {
                let data = state.data.get();
                let mine = notes_for_target(&data.notes, &kind_for_list, &id_for_list);
                let is_adding = draft.get().map(|d| d.edit_id.get_untracked().is_none()).unwrap_or(false);

                let empty = if mine.is_empty() && !is_adding {
                    Some(view! { <p class="notes-empty">"No notes yet."</p> })
                } else {
                    None
                };

                let items = mine
                    .into_iter()
                    .map(|note| {
                        let note = note.clone();
                        note_row(state, note, draft, error, pending)
                    })
                    .collect_view();

                // The add editor renders below the list when a NEW note is being
                // drafted (an edit editor renders inline within its row). Clone
                // the target into locals so this reactive closure stays `Fn`.
                let editor_kind = kind_for_list.clone();
                let editor_id = id_for_list.clone();
                let add_editor = move || {
                    draft.get().filter(|d| d.edit_id.get_untracked().is_none()).map(|d| {
                        note_editor(
                            state,
                            d,
                            editor_kind.clone(),
                            editor_id.clone(),
                            draft,
                            error,
                            pending,
                        )
                    })
                };

                view! {
                    {empty}
                    <ul class="notes-list">{items}</ul>
                    {add_editor}
                }
            }}

            {move || error.get().map(|msg| view! { <p class="notes-error">{msg}</p> })}
        </div>
    }
}

/// One note row: meta line (category badge, date, edit/delete) + body, or the
/// inline editor when this note is the one being edited.
fn note_row(
    state: AppState,
    note: Note,
    draft: RwSignal<Option<Draft>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
) -> impl IntoView {
    let id = note.id.clone();
    let target_kind = note.target.kind.clone();
    let target_id = note.target.id.clone();
    let category = note.category.clone();
    let body = note.body.clone();
    let when = format_when(&note.updated_at);

    let token_missing = move || state.mutation_token.get().is_none();

    // Is this row currently being edited?
    let row_id = id.clone();
    let editing = move || {
        draft
            .get()
            .map(|d| d.edit_id.get_untracked().as_deref() == Some(row_id.as_str()))
            .unwrap_or(false)
    };

    let start_edit = {
        let cat = category.clone();
        let bod = body.clone();
        let id = id.clone();
        move |_| {
            error.set(None);
            draft.set(Some(Draft {
                edit_id: create_rw_signal(Some(id.clone())),
                category: create_rw_signal(cat.clone()),
                body: create_rw_signal(bod.clone()),
            }));
        }
    };

    let delete_id = id.clone();
    let on_delete = move |_| {
        let payload = build_delete_payload(&delete_id);
        post(state, payload, draft, error, pending);
    };

    let editor_draft = move || draft.get().filter(|d| d.edit_id.get_untracked().as_deref() == Some(id.as_str()));

    view! {
        <li class=format!("note-item cat-{category}")>
            {move || if editing() {
                editor_draft()
                    .map(|d| note_editor(state, d, target_kind.clone(), target_id.clone(), draft, error, pending).into_view())
                    .unwrap_or_else(|| view! {}.into_view())
            } else {
                view! {
                    <div class="note-meta">
                        <span class=format!("note-cat cat-{category}")>{category.clone()}</span>
                        <span class="note-when">{when.clone()}</span>
                        <span class="note-actions">
                            <button class="notes-btn"
                                prop:disabled=move || pending.get() || token_missing()
                                on:click=start_edit.clone()
                            >"Edit"</button>
                            <button class="notes-btn"
                                prop:disabled=move || pending.get() || token_missing()
                                on:click=on_delete.clone()
                            >"Delete"</button>
                        </span>
                    </div>
                    <p class="note-body">{body.clone()}</p>
                }.into_view()
            }}
        </li>
    }
}

/// The category-select + body-textarea editor, shared by add and edit. On save
/// it mints id/timestamps (edit reuses the existing note's id + `createdAt`) and
/// posts the upsert.
fn note_editor(
    state: AppState,
    d: Draft,
    target_kind: String,
    target_id: String,
    draft: RwSignal<Option<Draft>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
) -> impl IntoView {
    let on_save = {
        let target_kind = target_kind.clone();
        let target_id = target_id.clone();
        move |_| {
            let body = d.body.get_untracked();
            if body.trim().is_empty() {
                return;
            }
            let category = d.category.get_untracked();
            let now = now_iso();
            // Edit: reuse id + preserve createdAt from the existing note. Add:
            // mint a fresh id and stamp createdAt == now.
            let (id, created_at) = match d.edit_id.get_untracked() {
                Some(existing_id) => {
                    let created = state
                        .data
                        .get_untracked()
                        .notes
                        .iter()
                        .find(|n| n.id == existing_id)
                        .map(|n| n.created_at.clone())
                        .unwrap_or_else(|| now.clone());
                    (existing_id, created)
                }
                None => (mint_note_id(), now.clone()),
            };
            let note = build_note(
                id,
                target_kind.clone(),
                target_id.clone(),
                category,
                body,
                created_at,
                now,
            );
            let payload = build_update_payload(&note);
            post(state, payload, draft, error, pending);
        }
    };

    let on_cancel = move |_| {
        draft.set(None);
        error.set(None);
    };

    view! {
        <div class="note-editor">
            <select class="note-editor__select"
                prop:disabled=move || pending.get()
                on:change=move |e| d.category.set(event_target_value(&e))
            >
                {CATEGORIES.iter().map(|(value, label)| {
                    let value = value.to_string();
                    let v = value.clone();
                    view! {
                        <option value=value.clone()
                            selected=move || d.category.get() == v
                        >{label.to_string()}</option>
                    }
                }).collect_view()}
            </select>
            <textarea class="note-editor__body"
                placeholder="What should a reader know about this element?"
                rows="3"
                prop:value=move || d.body.get()
                prop:disabled=move || pending.get()
                on:input=move |e| d.body.set(event_target_value(&e))
            />
            <div class="note-editor__actions">
                <button class="notes-btn notes-btn--primary"
                    prop:disabled=move || pending.get() || d.body.get().trim().is_empty()
                    on:click=on_save
                >{move || if pending.get() { "Saving…" } else { "Save" }}</button>
                <button class="notes-btn"
                    prop:disabled=move || pending.get()
                    on:click=on_cancel
                >"Cancel"</button>
            </div>
        </div>
    }
}

/// POST the payload; on success close the editor (the SSE `valid` event refreshes
/// the list), on failure surface the server's message inline. Mirrors the Rules
/// editor's `post_and_select` — the SSE-reloaded data is the single source of
/// truth, so we do not mutate local notes here.
fn post(
    state: AppState,
    payload: Value,
    draft: RwSignal<Option<Draft>>,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
) {
    let token = state.mutation_token.get_untracked();
    error.set(None);
    pending.set(true);
    spawn_local(async move {
        let result = post_mutation(token.as_deref(), "/api/notes", &payload).await;
        pending.set(false);
        match result {
            Ok(_) => {
                draft.set(None);
            }
            Err(err) => error.set(Some(err.message)),
        }
    });
}

/// Format an ISO timestamp as a short date, falling back to the raw string when
/// it doesn't parse (port of the JS `formatWhen`, simplified — we render the
/// date portion of the ISO string rather than localizing).
fn format_when(iso: &str) -> String {
    // ISO 8601 begins `YYYY-MM-DD`; show that prefix when present, else the raw.
    match iso.get(0..10) {
        Some(date) if date.len() == 10 && date.as_bytes()[4] == b'-' => date.to_string(),
        _ => iso.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, kind: &str, target_id: &str, updated: &str) -> Note {
        Note {
            id: id.into(),
            target: NoteTarget { kind: kind.into(), id: target_id.into() },
            category: "note".into(),
            body: "body".into(),
            author: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: updated.into(),
        }
    }

    #[test]
    fn notes_for_target_filters_by_kind_and_id() {
        let notes = vec![
            note("note-a", "node", "svc", "2026-01-02T00:00:00.000Z"),
            note("note-b", "node", "other", "2026-01-03T00:00:00.000Z"),
            note("note-c", "flow", "svc", "2026-01-04T00:00:00.000Z"),
        ];
        let mine = notes_for_target(&notes, "node", "svc");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].id, "note-a");
    }

    #[test]
    fn notes_for_target_sorts_newest_first() {
        let notes = vec![
            note("note-old", "node", "svc", "2026-01-01T00:00:00.000Z"),
            note("note-new", "node", "svc", "2026-03-01T00:00:00.000Z"),
            note("note-mid", "node", "svc", "2026-02-01T00:00:00.000Z"),
        ];
        let mine = notes_for_target(&notes, "node", "svc");
        let ids: Vec<&str> = mine.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["note-new", "note-mid", "note-old"]);
    }

    #[test]
    fn build_note_trims_body_and_emits_no_author() {
        let note = build_note(
            "note-x".into(),
            "node".into(),
            "svc".into(),
            "caveat".into(),
            "  hello  ".into(),
            "2026-01-01T00:00:00.000Z".into(),
            "2026-01-02T00:00:00.000Z".into(),
        );
        assert_eq!(note.body, "hello");
        assert_eq!(note.author, None);
    }

    #[test]
    fn update_payload_carries_action_and_full_note_with_no_extra_fields() {
        let note = build_note(
            "note-x".into(),
            "node".into(),
            "svc".into(),
            "todo".into(),
            "do the thing".into(),
            "2026-01-01T00:00:00.000Z".into(),
            "2026-01-02T00:00:00.000Z".into(),
        );
        let payload = build_update_payload(&note);
        assert_eq!(payload["action"], "update");
        let obj = payload["note"].as_object().unwrap();
        // Exactly the schema-permitted fields, no `author` (None is skipped).
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec!["body", "category", "createdAt", "id", "target", "updatedAt"]
        );
        assert_eq!(payload["note"]["target"]["kind"], "node");
        assert_eq!(payload["note"]["target"]["id"], "svc");
        // target carries exactly kind + id.
        let target_keys: Vec<&String> =
            payload["note"]["target"].as_object().unwrap().keys().collect();
        assert_eq!(target_keys.len(), 2);
    }

    #[test]
    fn delete_payload_matches_contract() {
        assert_eq!(
            build_delete_payload("note-x"),
            json!({ "action": "delete", "id": "note-x" })
        );
    }

    #[test]
    fn base36_renders_lowercase_alnum() {
        assert_eq!(to_base36(0), "0");
        assert_eq!(to_base36(35), "z");
        assert_eq!(to_base36(36), "10");
        // A realistic epoch-ms value: lowercase alphanumeric, valid id suffix.
        let s = to_base36(1_750_000_000_000);
        assert!(s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        let id = format!("note-{s}");
        assert!(id.starts_with("note-"));
    }

    #[test]
    fn format_when_shows_date_prefix_or_raw() {
        assert_eq!(format_when("2026-06-18T12:34:56.000Z"), "2026-06-18");
        assert_eq!(format_when("not-a-date"), "not-a-date");
    }
}
