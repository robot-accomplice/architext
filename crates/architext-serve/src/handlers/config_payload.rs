//! Handler for `GET /api/config`.
//!
//! Reads the user-global config (`~/.architext/config.json`) and the
//! project config (`docs/architext/config.json`), resolves them through
//! `diagram_config::resolve_diagram_config_from_json`, and returns
//! `{ diagram, warnings, fields, sections }`.
//!
//! Port of `diagramConfigGetPayload(target)` in
//! `src/adapters/http/diagram-config-api.mjs`.
//!
//! Cache-Control: no-store (matches JS comment: "Never cache").

use std::path::{Path, PathBuf};

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use serde_json::{json, Value};

use architext_routing::diagram_config::{
    diagram_config_fields_json, resolve_diagram_config_from_json, section_labels_json,
};

use crate::AppState;

// ─── Path helpers ─────────────────────────────────────────────────────────────

/// `~/.architext/config.json`
fn user_config_path() -> Option<PathBuf> {
    dirs_home().map(|h| h.join(".architext").join("config.json"))
}

/// `<target>/docs/architext/config.json`
fn project_config_path(target: &Path) -> PathBuf {
    target.join("docs").join("architext").join("config.json")
}

/// Resolve the home directory.
///
/// Uses the `HOME` environment variable (matching Node's `os.homedir()`
/// on Unix). Falls back to `/` on failure.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("/")))
}

// ─── JSON config file reader ──────────────────────────────────────────────────

/// Read an optional config JSON file. Returns `(value, warnings)`.
///
/// Port of `readJsonLayer(file, source, ...)` — absent file is normal (None returned),
/// unreadable/malformed file degrades to a warning.
fn read_json_layer(path: &Path, source: &str) -> (Option<Value>, Vec<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (None, vec![]);
        }
        Err(e) => {
            return (
                None,
                vec![format!(
                    "{source}: could not read {} ({e}); ignored.",
                    path.display()
                )],
            );
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => (Some(v), vec![]),
        Err(e) => (
            None,
            vec![format!(
                "{source}: {} is not valid JSON ({e}); ignored.",
                path.display()
            )],
        ),
    }
}

// ─── Public config payload builder ───────────────────────────────────────────

/// Build the `/api/config` response payload.
///
/// Port of `diagramConfigGetPayload(target)`.
/// Returns `{ diagram: <resolved>, warnings: [...], fields: DIAGRAM_CONFIG_FIELDS, sections: SECTION_LABELS }`.
pub fn build_config_payload(target: &Path) -> Value {
    let user_config_path = user_config_path().unwrap_or_else(|| PathBuf::from("/nonexistent"));
    let project_config_path = project_config_path(target);

    let mut all_warnings: Vec<String> = Vec::new();

    let (user_raw, user_warnings) = read_json_layer(&user_config_path, "user config");
    all_warnings.extend(user_warnings);

    let (project_raw, project_warnings) = read_json_layer(&project_config_path, "project config");
    all_warnings.extend(project_warnings);

    let null = Value::Null;
    let user_val = user_raw.as_ref().unwrap_or(&null);
    let project_val = project_raw.as_ref().unwrap_or(&null);

    let layers: Vec<(&Value, &str)> = vec![
        (user_val, "user config"),
        (project_val, "project config"),
    ];

    let (config, resolve_warnings) = resolve_diagram_config_from_json(&layers);
    all_warnings.extend(resolve_warnings);

    json!({
        "diagram": config,
        "warnings": all_warnings,
        "fields": diagram_config_fields_json(),
        "sections": section_labels_json()
    })
}

// ─── HTTP handler ─────────────────────────────────────────────────────────────

/// GET /api/config → `{ diagram, warnings, fields, sections }` + Cache-Control: no-store
pub async fn get_config(Extension(state): Extension<AppState>) -> Response {
    let payload = build_config_payload(&state.target_dir);

    let mut headers = HeaderMap::new();
    headers.insert(
        "cache-control",
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );

    let body = serde_json::to_string_pretty(&payload)
        .map(|s| format!("{s}\n"))
        .unwrap_or_else(|_| "{}\n".to_string());

    (StatusCode::OK, headers, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_json(dir: &Path, rel: &str, v: &Value) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
    }

    #[test]
    fn payload_shape_no_config_files() {
        let td = temp_dir();
        let payload = build_config_payload(td.path());
        // shape: diagram, warnings, fields, sections
        assert!(payload.get("diagram").is_some(), "missing diagram");
        assert!(payload.get("warnings").is_some(), "missing warnings");
        assert!(payload.get("fields").is_some(), "missing fields");
        assert!(payload.get("sections").is_some(), "missing sections");
    }

    #[test]
    fn payload_diagram_defaults_when_no_config() {
        let td = temp_dir();
        let payload = build_config_payload(td.path());
        assert_eq!(payload["diagram"]["layout"]["laneWidth"].as_f64(), Some(210.0));
        assert_eq!(payload["diagram"]["zoom"]["minFitZoom"].as_f64(), Some(0.15));
        assert_eq!(payload["diagram"]["legibility"]["gapArrowheads"].as_f64(), Some(0.5));
        // warnings should be empty
        let warnings = payload["warnings"].as_array().unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn payload_fields_has_all_sections() {
        let td = temp_dir();
        let payload = build_config_payload(td.path());
        for section in &["layout", "sequence", "zoom", "legibility"] {
            assert!(
                payload["fields"].get(section).is_some(),
                "fields missing section {section}"
            );
        }
    }

    #[test]
    fn payload_sections_correct_labels() {
        let td = temp_dir();
        let payload = build_config_payload(td.path());
        assert_eq!(payload["sections"]["layout"].as_str(), Some("Layout & spacing"));
        assert_eq!(payload["sections"]["zoom"].as_str(), Some("Fit zoom"));
    }

    #[test]
    fn project_config_overrides_defaults() {
        let td = temp_dir();
        write_json(
            td.path(),
            "docs/architext/config.json",
            &json!({ "layout": { "laneWidth": 400 } }),
        );
        let payload = build_config_payload(td.path());
        assert_eq!(payload["diagram"]["layout"]["laneWidth"].as_f64(), Some(400.0));
        // other fields unchanged
        assert_eq!(payload["diagram"]["layout"]["rowGap"].as_f64(), Some(102.0));
        let warnings = payload["warnings"].as_array().unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn malformed_project_config_degrades_gracefully() {
        let td = temp_dir();
        let config_dir = td.path().join("docs").join("architext");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), b"not json {").unwrap();
        let payload = build_config_payload(td.path());
        // defaults still served
        assert_eq!(payload["diagram"]["layout"]["laneWidth"].as_f64(), Some(210.0));
        // warning about parse failure
        let warnings = payload["warnings"].as_array().unwrap();
        assert!(!warnings.is_empty());
    }
}
