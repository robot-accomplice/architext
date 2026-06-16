//! Handler for `POST /api/config`.
//!
//! Port of `writeDiagramConfig` from `src/adapters/http/diagram-config-api.mjs`.
//!
//! Request body (JSON, max 1 MiB):
//!   `{ scope: "user"|"project", diagram: { layout: {...}, ... } }`
//!
//! Behaviour:
//!   1. Normalize + clamp the incoming `diagram` through `normalizeDiagramConfigLayer`
//!      (via `resolve_diagram_config_from_json` with a single layer).
//!   2. Diff against defaults → write only the non-default overrides to disk.
//!   3. Write to `~/.architext/config.json` (scope="user") or
//!      `<target>/docs/architext/config.json` (scope="project").
//!   4. Re-resolve the full config (user + project layers) and return the
//!      `/api/config` payload `{ ok, scope, file, written, diagram, warnings }`.
//!   5. Refresh the plan farm after write (re-key with new layout).
//!
//! Success response (HTTP 200):
//!   `{ ok: true, scope, file, written, diagram, warnings }`
//!
//! Failure response (HTTP 200):
//!   `{ ok: false, mode: "config", error: "...", reload: false }`
//!
//! Notes on JS semantics:
//! - `writeDiagramConfig` does NOT go through the write-lock or write-set.
//!   The config files are outside the data directory and are not covered by
//!   the data-write transaction. The JS source uses a plain `writeFile` call
//!   (no lock, no rollback). We mirror that: no `WriteSet` here.
//! - The farm refresh happens after write; it is best-effort (log + continue on error).

use std::path::{Path, PathBuf};

use axum::{body::Bytes, response::Response, Extension};
use serde_json::{json, Value};

use architext_routing::diagram_config::{
    diff_diagram_config_from_defaults, resolve_diagram_config_from_json,
};

use crate::farm_state::{refresh_farm, Farm};
use crate::handlers::config_payload::build_config_payload;
use crate::handlers::rules::MAX_REQUEST_BODY_BYTES;
use crate::AppState;

// ─── Path helpers ─────────────────────────────────────────────────────────────

fn user_config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .map(|h| h.join(".architext").join("config.json"))
}

fn project_config_path(target: &Path) -> PathBuf {
    target.join("docs").join("architext").join("config.json")
}

fn config_path_for_scope(scope: &str, target: &Path) -> Result<PathBuf, String> {
    match scope {
        "user" => user_config_path().ok_or_else(|| "Cannot determine home directory".to_string()),
        "project" => Ok(project_config_path(target)),
        other => Err(format!(
            "Unknown diagram config scope \"{other}\" (expected \"project\" or \"user\")."
        )),
    }
}

// ─── Handler ──────────────────────────────────────────────────────────────────

/// POST /api/config
pub async fn post_config(
    Extension(state): Extension<AppState>,
    Extension(farm): Extension<Farm>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response("config", "Request body is too large");
    }

    let payload: Value = match serde_json::from_slice(if body.is_empty() { b"{}" } else { &body }) {
        Ok(v) => v,
        Err(e) => return error_response("config", &format!("Invalid JSON: {e}")),
    };

    match write_diagram_config(&state, &farm, &payload).await {
        Ok(body) => ok_response(body),
        Err(msg) => error_response("config", &msg),
    }
}

async fn write_diagram_config(
    state: &AppState,
    farm: &Farm,
    payload: &Value,
) -> Result<Value, String> {
    let scope = payload["scope"].as_str().unwrap_or("project");
    let diagram = &payload["diagram"];

    // Normalize + clamp the incoming diagram value.
    // `resolve_diagram_config_from_json` with a single layer gives us
    // the clamped, full-default-filled config.
    let null = Value::Null;
    let diagram_val = if diagram.is_null() { &null } else { diagram };
    let (normalized, _warnings) =
        resolve_diagram_config_from_json(&[(diagram_val, &format!("{scope} config"))]);

    // Reduce to only non-default overrides (mirrors `diffDiagramConfigFromDefaults`).
    let overrides = diff_diagram_config_from_defaults(&normalized);

    // Resolve the file path for the scope.
    let file = config_path_for_scope(scope, &state.target_dir)?;

    // Ensure parent directory exists.
    if let Some(parent) = file.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Could not create config directory: {e}"))?;
    }

    // Write — `JSON.stringify(overrides, null, 2) + "\n"` via write_json_string.
    let contents = architext_core::json_write::write_json_string(&overrides);
    tokio::fs::write(&file, contents.as_bytes())
        .await
        .map_err(|e| format!("Could not write config file: {e}"))?;

    // Re-resolve the full config (user + project layers) — mirrors
    // `loadDiagramConfig(target, { homedir })` in the JS handler.
    let resolved_payload = build_config_payload(&state.target_dir);

    // Refresh the plan farm with the new config.
    let data_dir = state.data_dir.clone();
    let farm_clone = farm.clone();
    tokio::task::spawn_blocking(move || {
        refresh_farm(&farm_clone, &data_dir);
    });

    Ok(json!({
        "ok": true,
        "scope": scope,
        "file": file.to_string_lossy(),
        "written": overrides,
        "diagram": resolved_payload["diagram"],
        "warnings": resolved_payload["warnings"]
    }))
}

// ─── Response helpers ─────────────────────────────────────────────────────────

fn ok_response(body: Value) -> Response {
    use axum::http::{HeaderValue, StatusCode};
    let s = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
    let mut resp = Response::new(axum::body::Body::from(s));
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp
}

fn error_response(mode: &str, msg: &str) -> Response {
    ok_response(json!({
        "ok": false,
        "mode": mode,
        "error": msg,
        "reload": false
    }))
}
