//! One config-editor control, rendered from a [`FieldSpec`] (no hardcoded field
//! list — the spec comes from the `/api/config` `fields` payload).
//!
//! The draft value is held as a `RwSignal<String>` (uniform across kinds), so
//! the panel can shape every section into one payload without per-kind signal
//! plumbing. The renderer picks the control by [`FieldKind`]:
//!   - `Number` → `<input type=number>` honoring min/max/step;
//!   - `Select` → `<select>` over the spec's options;
//!   - `Bool`   → a checkbox round-tripping the string `"true"`/`"false"`.
//!
//! On-language per DESIGN.md: hairline inputs, mono for numeric values, the
//! field label as a mono overline. Layout/styling reuse the `.config-field`
//! classes (see styles.css).

use leptos::*;

use crate::data::models::{FieldKind, FieldSpec};

/// Render the control for one field, bound to `value`. The unit (if any) is
/// shown as a trailing mono hint so the user sees `px` / `×` / `arrowheads`.
pub fn config_field_view(spec: FieldSpec, value: RwSignal<String>) -> View {
    let label = spec.label.clone();
    let unit = spec.unit.clone();

    let control = match &spec.kind {
        FieldKind::Number { min, max, step } => number_control(value, *min, *max, *step),
        FieldKind::Select { options } => select_control(value, options.clone()),
        FieldKind::Bool => bool_control(value),
    };

    view! {
        <label class="config-field">
            <span class="config-field__label">
                {label}
                {unit.map(|u| view! { <span class="config-field__unit mono">{u}</span> })}
            </span>
            {control}
        </label>
    }
    .into_view()
}

/// `<input type=number>` honoring the spec range/step; mono (a numeric value).
fn number_control(
    value: RwSignal<String>,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
) -> View {
    // `js_number_to_string`-free: the spec numbers print cleanly via Rust's
    // default float formatting for the attribute values.
    let fmt = |n: f64| {
        // Render integers without a trailing `.0` so `step=2` not `step=2`.
        if n.fract() == 0.0 { format!("{n:.0}") } else { n.to_string() }
    };
    view! {
        <input
            class="config-field__input mono"
            type="number"
            min=min.map(fmt)
            max=max.map(fmt)
            step=step.map(fmt)
            prop:value=move || value.get()
            on:input=move |e| value.set(event_target_value(&e))
        />
    }
    .into_view()
}

/// `<select>` over the spec options; the current draft value selects an option.
fn select_control(value: RwSignal<String>, options: Vec<String>) -> View {
    view! {
        <select
            class="config-field__input"
            on:change=move |e| value.set(event_target_value(&e))
        >
            {options
                .into_iter()
                .map(|opt| {
                    let opt_value = opt.clone();
                    view! {
                        <option
                            value=opt_value.clone()
                            selected=move || value.get() == opt_value
                        >{opt}</option>
                    }
                })
                .collect_view()}
        </select>
    }
    .into_view()
}

/// A checkbox round-tripping the string `"true"`/`"false"`.
fn bool_control(value: RwSignal<String>) -> View {
    view! {
        <input
            class="config-field__check"
            type="checkbox"
            prop:checked=move || value.get() == "true"
            on:change=move |e| {
                value.set(if event_target_checked(&e) { "true" } else { "false" }.to_string());
            }
        />
    }
    .into_view()
}

/// The initial draft string for a field: the resolved `diagram` value for that
/// `(section, field)`, falling back to the spec default. Numbers/bools are
/// stringified uniformly so a single `RwSignal<String>` backs every kind.
pub fn initial_field_value(diagram: &serde_json::Value, section: &str, spec: &FieldSpec) -> String {
    let resolved = diagram.get(section).and_then(|s| s.get(&spec.key));
    let value = resolved.unwrap_or(&spec.default);
    value_to_string(value)
}

/// Stringify a config JSON scalar for a draft signal (number → its JSON text,
/// bool → `"true"`/`"false"`, string → itself, anything else → empty).
pub fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn initial_value_prefers_resolved_over_default() {
        let diagram = json!({ "layout": { "laneWidth": 300 } });
        let spec = FieldSpec {
            key: "laneWidth".into(),
            label: "Column width".into(),
            kind: FieldKind::Number { min: Some(60.0), max: Some(800.0), step: Some(2.0) },
            default: json!(210),
            unit: Some("px".into()),
        };
        assert_eq!(initial_field_value(&diagram, "layout", &spec), "300");
    }

    #[test]
    fn initial_value_falls_back_to_default_when_absent() {
        let diagram = json!({ "layout": {} });
        let spec = FieldSpec {
            key: "rowGap".into(),
            label: "Row gap".into(),
            kind: FieldKind::Number { min: None, max: None, step: None },
            default: json!(102),
            unit: Some("px".into()),
        };
        assert_eq!(initial_field_value(&diagram, "layout", &spec), "102");
    }

    #[test]
    fn value_to_string_handles_scalar_kinds() {
        assert_eq!(value_to_string(&json!(0.15)), "0.15");
        assert_eq!(value_to_string(&json!(true)), "true");
        assert_eq!(value_to_string(&json!("orthogonal")), "orthogonal");
        assert_eq!(value_to_string(&json!(null)), "");
    }
}
