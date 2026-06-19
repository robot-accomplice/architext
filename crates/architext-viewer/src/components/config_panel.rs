//! Editable diagram-config drawer (a right-side drawer over the shell).
//!
//! Renders a control per field, grouped by section, from the `/api/config`
//! `fields`/`sections` spec (single source — never a hardcoded field list),
//! pre-filled from the resolved `diagram` values. A scope segmented control
//! chooses the layer to write (Project = `docs/architext/config.json`, the
//! default; User = `~/.architext/config.json`). Save posts
//! `{ scope, diagram }` to `POST /api/config` via [`crate::data::post_mutation`].
//!
//! The server normalizes/clamps the layer, diffs it from defaults, writes the
//! chosen file, re-resolves the full config, and refreshes the plan farm; the
//! response is the new `{ diagram, warnings }`. Config lives OUTSIDE the watched
//! data dir, so a write does NOT fire the data-events SSE — the drawer must
//! update from the POST response itself. On success we fold the response back
//! through [`crate::state::AppState::set_config`], which both re-prefills the
//! controls with the clamped/resolved values AND reflows the diagrams (the
//! canvas plan key folds in the config identity; the farm was just refreshed,
//! so the new plan is served).
//!
//! Reset to defaults posts an empty layer (`diagram: {}`) → the server writes
//! `{}` and re-resolves to built-in defaults.

use std::collections::HashMap;

use leptos::*;
use serde_json::{json, Map, Value};

use crate::components::config_field::{config_field_view, initial_field_value};
use crate::data::models::{ConfigPayload, ConfigSection, FieldKind};
use crate::data::post_mutation;
use crate::state::{use_app_state, AppState};

/// Write scopes, in display order. Project is the default (the team-shared,
/// in-repo layer); User is the personal global layer.
const SCOPES: &[(&str, &str)] = &[("project", "Project"), ("user", "User")];

#[component]
pub fn ConfigPanel(
    /// Whether the drawer is open. Owned by the header; the close button writes it.
    #[prop(into)] open: RwSignal<bool>,
) -> impl IntoView {
    let state = use_app_state();

    view! {
        <Show when=move || open.get() fallback=|| ()>
            // Scrim closes the drawer on click-away.
            <div class="config-scrim" on:click=move |_| open.set(false)></div>
            <aside class="config-drawer" role="dialog" aria-label="Diagram configuration">
                <div class="config-drawer__head">
                    <div>
                        <div class="overline">"DIAGRAM CONFIG"</div>
                        <h2 class="config-drawer__title">"Edit configuration"</h2>
                    </div>
                    <button
                        class="panel-collapse-toggle"
                        title="Close config"
                        on:click=move |_| open.set(false)
                    >"✕"</button>
                </div>
                // The form is rebuilt whenever the resolved config changes (e.g.
                // after a successful save re-prefills the clamped values), so the
                // draft signals re-seed from the current resolved `diagram`.
                {move || {
                    let data = state.data.get();
                    match data.config.as_ref() {
                        Some(config) if !config.sections_spec().is_empty() => {
                            editor_form(state, config.clone()).into_view()
                        }
                        _ => view! {
                            <p class="config-drawer__empty">"No diagram config spec available."</p>
                        }.into_view(),
                    }
                }}
            </aside>
        </Show>
    }
}

/// Build the editor form for a resolved config payload. Draft signals are seeded
/// fresh from the resolved `diagram` each time this runs (i.e. on every config
/// change), so the controls always reflect the current resolved values.
fn editor_form(state: AppState, config: ConfigPayload) -> impl IntoView {
    let sections = config.sections_spec();
    // One string-backed draft per `(section, field)`, keyed by the spec.
    let drafts: HashMap<(String, String), RwSignal<String>> = sections
        .iter()
        .flat_map(|section| {
            section.fields.iter().map(|spec| {
                let key = (section.id.clone(), spec.key.clone());
                let initial = initial_field_value(&config.diagram, &section.id, spec);
                (key, create_rw_signal(initial))
            })
        })
        .collect();

    // Scope (Project default), inline status, and the resolved warnings to show.
    let scope = create_rw_signal("project".to_string());
    let error = create_rw_signal::<Option<String>>(None);
    let pending = create_rw_signal(false);
    let warnings = create_rw_signal::<Vec<String>>(config.warnings.clone());

    let token_missing = move || state.mutation_token.get().is_none();

    // ── Save: shape the edited layer and POST it. ─────────────────────────────
    let sections_for_save = sections.clone();
    let drafts_for_save = drafts.clone();
    let on_save = move |_| {
        let diagram = build_edited_layer(&sections_for_save, &drafts_for_save);
        post_config(state, scope.get_untracked(), diagram, error, pending, warnings);
    };

    // ── Reset to defaults: post an empty layer (server writes `{}`). ──────────
    let on_reset = move |_| {
        post_config(state, scope.get_untracked(), json!({}), error, pending, warnings);
    };

    view! {
        <div class="config-editor">
            // Scope segmented control.
            <div class="config-editor__scope">
                <span class="config-field__label">"Write to"</span>
                <div class="config-seg" role="radiogroup" aria-label="Config scope">
                    {SCOPES.iter().map(|(value, label)| {
                        let value = value.to_string();
                        let active_value = value.clone();
                        let checked_value = value.clone();
                        view! {
                            <button
                                class="config-seg__btn"
                                class:is-active=move || scope.get() == active_value
                                role="radio"
                                aria-checked=move || (scope.get() == checked_value).to_string()
                                on:click=move |_| scope.set(value.clone())
                            >{*label}</button>
                        }
                    }).collect_view()}
                </div>
                <p class="config-editor__scope-hint mono">
                    {move || match scope.get().as_str() {
                        "user" => "~/.architext/config.json",
                        _ => "docs/architext/config.json",
                    }}
                </p>
            </div>

            // Sections of controls.
            {sections.into_iter().map(|section| {
                section_view(section, &drafts)
            }).collect_view()}

            // Warnings (unknown keys / clamped values) from the resolved payload.
            {move || {
                let w = warnings.get();
                (!w.is_empty()).then(|| view! {
                    <ul class="config-editor__warnings">
                        {w.into_iter().map(|msg| view! {
                            <li class="config-editor__warning">{msg}</li>
                        }).collect_view()}
                    </ul>
                })
            }}

            // Inline error (rejected/rolled-back write) or token-missing notice.
            {move || error.get().map(|msg| view! {
                <p class="config-editor__error">{msg}</p>
            })}
            {move || token_missing().then(|| view! {
                <p class="config-editor__error">
                    "Editing is not authorized in this session (no mutation token)."
                </p>
            })}

            <div class="config-editor__actions">
                <button class="rule-editor__btn rule-editor__btn--primary"
                    prop:disabled=move || pending.get() || token_missing()
                    on:click=on_save
                >{move || if pending.get() { "Saving…" } else { "Save" }}</button>
                <button class="rule-editor__btn"
                    prop:disabled=move || pending.get() || token_missing()
                    on:click=on_reset
                >"Reset to defaults"</button>
            </div>
        </div>
    }
}

/// One section: a mono section label + its field controls.
fn section_view(
    section: ConfigSection,
    drafts: &HashMap<(String, String), RwSignal<String>>,
) -> View {
    let controls = section
        .fields
        .iter()
        .filter_map(|spec| {
            let value = *drafts.get(&(section.id.clone(), spec.key.clone()))?;
            Some(config_field_view(spec.clone(), value))
        })
        .collect_view();
    view! {
        <fieldset class="config-section">
            <legend class="config-section__label overline">{section.label}</legend>
            <div class="config-section__fields">{controls}</div>
        </fieldset>
    }
    .into_view()
}

/// Shape the edited drafts into a `diagram` layer (`{ section: { field: value }}`)
/// for the POST body. Numbers parse to JSON numbers, bools to JSON bools,
/// selects/strings pass through; an empty or unparseable numeric draft is
/// dropped so it falls through to a lower layer (the server re-clamps anyway).
fn build_edited_layer(
    sections: &[ConfigSection],
    drafts: &HashMap<(String, String), RwSignal<String>>,
) -> Value {
    let mut out = Map::new();
    for section in sections {
        let mut section_map = Map::new();
        for spec in &section.fields {
            let Some(signal) = drafts.get(&(section.id.clone(), spec.key.clone())) else {
                continue;
            };
            let raw = signal.get_untracked();
            let raw = raw.trim();
            let value = match &spec.kind {
                FieldKind::Number { .. } => match raw.parse::<f64>() {
                    Ok(n) if n.is_finite() => json!(n),
                    // Drop an empty/invalid number → falls through to defaults.
                    _ => continue,
                },
                FieldKind::Bool => json!(raw == "true"),
                FieldKind::Select { .. } => {
                    if raw.is_empty() {
                        continue;
                    }
                    json!(raw)
                }
            };
            section_map.insert(spec.key.clone(), value);
        }
        if !section_map.is_empty() {
            out.insert(section.id.clone(), Value::Object(section_map));
        }
    }
    Value::Object(out)
}

/// POST `{ scope, diagram }` to `/api/config`, then fold the response back into
/// `AppState` (re-prefill + reflow) and surface the re-resolved warnings. A
/// rejected write surfaces the server's message inline; the diagrams keep their
/// last-good layout (no `set_config`).
fn post_config(
    state: AppState,
    scope: String,
    diagram: Value,
    error: RwSignal<Option<String>>,
    pending: RwSignal<bool>,
    warnings: RwSignal<Vec<String>>,
) {
    let token = state.mutation_token.get_untracked();
    let payload = json!({ "scope": scope, "diagram": diagram });
    error.set(None);
    pending.set(true);
    spawn_local(async move {
        let result = post_mutation(token.as_deref(), "/api/config", &payload).await;
        pending.set(false);
        match result {
            Ok(value) => {
                // The success body is `{ ok, scope, file, written, diagram,
                // warnings }`. Parse the re-resolved config slice and push it to
                // AppState — this re-prefills the controls (the form rebuilds on
                // the config change) and reflows the diagrams. `fields`/`sections`
                // are absent in the POST body and default to empty; we re-attach
                // the spec the editor already holds so the controls keep rendering.
                match serde_json::from_value::<ConfigPayload>(value) {
                    Ok(mut next) => {
                        warnings.set(next.warnings.clone());
                        attach_spec(&mut next, state);
                        state.set_config(next);
                    }
                    Err(e) => error.set(Some(format!("Could not parse config response: {e}"))),
                }
            }
            Err(err) => error.set(Some(err.message)),
        }
    });
}

/// The POST response omits `fields`/`sections`; carry them over from the config
/// the editor currently holds so the rebuilt form keeps its control spec.
fn attach_spec(next: &mut ConfigPayload, state: AppState) {
    if let Some(current) = state.data.get_untracked().config.as_ref() {
        if next.fields.is_null() || next.fields.as_object().map(Map::is_empty).unwrap_or(false) {
            next.fields = current.fields.clone();
        }
        if next.sections.is_null()
            || next.sections.as_object().map(Map::is_empty).unwrap_or(false)
        {
            next.sections = current.sections.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{FieldKind, FieldSpec};

    fn section(id: &str, fields: Vec<FieldSpec>) -> ConfigSection {
        ConfigSection { id: id.into(), label: id.into(), fields }
    }

    fn num_field(key: &str) -> FieldSpec {
        FieldSpec {
            key: key.into(),
            label: key.into(),
            kind: FieldKind::Number { min: Some(0.0), max: Some(1000.0), step: Some(2.0) },
            default: json!(0),
            unit: Some("px".into()),
        }
    }

    fn drafts_of(pairs: &[(&str, &str, &str)]) -> HashMap<(String, String), RwSignal<String>> {
        // NOTE: create_rw_signal requires a reactive runtime; build the map with
        // plain values via a leptos runtime in the test harness.
        let mut map = HashMap::new();
        for (section, field, value) in pairs {
            map.insert(
                (section.to_string(), field.to_string()),
                create_rw_signal(value.to_string()),
            );
        }
        map
    }

    #[test]
    fn build_edited_layer_shapes_numbers_by_section() {
        let runtime = create_runtime();
        let sections = vec![section("layout", vec![num_field("laneWidth"), num_field("rowGap")])];
        let drafts = drafts_of(&[("layout", "laneWidth", "300"), ("layout", "rowGap", "120")]);
        let layer = build_edited_layer(&sections, &drafts);
        assert_eq!(layer["layout"]["laneWidth"].as_f64(), Some(300.0));
        assert_eq!(layer["layout"]["rowGap"].as_f64(), Some(120.0));
        runtime.dispose();
    }

    #[test]
    fn build_edited_layer_drops_empty_numbers() {
        let runtime = create_runtime();
        let sections = vec![section("layout", vec![num_field("laneWidth"), num_field("rowGap")])];
        // rowGap left blank → dropped so it falls through to a lower layer.
        let drafts = drafts_of(&[("layout", "laneWidth", "300"), ("layout", "rowGap", "")]);
        let layer = build_edited_layer(&sections, &drafts);
        assert_eq!(layer["layout"]["laneWidth"].as_f64(), Some(300.0));
        assert!(layer["layout"].get("rowGap").is_none());
        runtime.dispose();
    }

    #[test]
    fn build_edited_layer_handles_bool_and_select() {
        let runtime = create_runtime();
        let bool_field = FieldSpec {
            key: "snap".into(),
            label: "snap".into(),
            kind: FieldKind::Bool,
            default: json!(false),
            unit: None,
        };
        let select_field = FieldSpec {
            key: "style".into(),
            label: "style".into(),
            kind: FieldKind::Select { options: vec!["a".into(), "b".into()] },
            default: json!("a"),
            unit: None,
        };
        let sections = vec![section("opt", vec![bool_field, select_field])];
        let drafts = drafts_of(&[("opt", "snap", "true"), ("opt", "style", "b")]);
        let layer = build_edited_layer(&sections, &drafts);
        assert_eq!(layer["opt"]["snap"].as_bool(), Some(true));
        assert_eq!(layer["opt"]["style"].as_str(), Some("b"));
        runtime.dispose();
    }

    #[test]
    fn build_edited_layer_drops_empty_sections() {
        let runtime = create_runtime();
        // A section whose only field is a blank number contributes nothing.
        let sections = vec![section("layout", vec![num_field("laneWidth")])];
        let drafts = drafts_of(&[("layout", "laneWidth", "")]);
        let layer = build_edited_layer(&sections, &drafts);
        assert!(layer.as_object().unwrap().is_empty());
        runtime.dispose();
    }
}
