//! Handler for `POST /api/rules`.
//!
//! Port of `updateRulesRequest` / `updateRulesRequestUnlocked` from
//! `src/adapters/http/rules-api.mjs`.
//!
//! Request body (JSON, max 1 MiB):
//!   `{ action?: "update"|"delete"|"move"|"move-before", ...action-specific fields }`
//!
//! Success response (HTTP 200):
//!   `{ rules: [...], validation: { ok: true, ... } }`
//!
//! Failure response (HTTP 200, same status code as JS):
//!   `{ ok: false, mode: "rules", error: "<message>", reload: false }`
//!
//! The handler acquires the per-process write-lock, snapshots the file,
//! applies the domain mutation, writes via `write_json_string`, runs the
//! Rust validator, and rolls back on failure.

use axum::{
    body::Bytes,
    response::Response,
    Extension,
};
use serde_json::{json, Value};

use architext_core::domain::rules::{delete_rule, move_rule, move_rule_before, upsert_rule};
use architext_core::json_write::write_json_string;

use crate::write_txn::WriteSet;
use crate::AppState;

/// Maximum POST body size: 1 MiB (port of JS `maxRequestBodyBytes = 1024 * 1024`).
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// POST /api/rules
pub async fn post_rules(
    Extension(state): Extension<AppState>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response("rules", "Request body is too large");
    }

    let payload: Value = match serde_json::from_slice(if body.is_empty() { b"{}" } else { &body }) {
        Ok(v) => v,
        Err(e) => return error_response("rules", &format!("Invalid JSON: {e}")),
    };

    let lock = state.write_lock.lock().await;
    let result = update_rules_unlocked(&state, &payload).await;
    drop(lock);

    match result {
        Ok(body) => ok_response(body),
        Err(msg) => error_response("rules", &msg),
    }
}

async fn update_rules_unlocked(state: &AppState, payload: &Value) -> Result<Value, String> {
    let data_dir = &state.data_dir;

    // Read manifest to find rules path
    let manifest_path = data_dir.join("manifest.json");
    let manifest_text = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| format!("Could not read manifest.json: {e}"))?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("Could not parse manifest.json: {e}"))?;

    let rules_rel = manifest["files"]["rules"]
        .as_str()
        .ok_or_else(|| "Rules editing requires manifest.files.rules".to_string())?;
    let rules_path = data_dir.join(rules_rel);

    // Read current rules document
    let rules_text = tokio::fs::read_to_string(&rules_path)
        .await
        .map_err(|e| format!("Could not read rules file: {e}"))?;
    let rules_document: Value = serde_json::from_str(&rules_text)
        .map_err(|e| format!("Could not parse rules file: {e}"))?;

    // Apply domain action
    let next_document = apply_rules_action(&rules_document, payload)?;

    // Transactional write: capture → write → validate → restore on failure
    let mut write_set = WriteSet::new();

    let write_result: Result<(), String> = async {
        write_set
            .write(&rules_path, &write_json_string(&next_document))
            .await
            .map_err(|e| format!("Could not write rules file: {e}"))?;

        let schema_dir = state.schema_dir.as_path();
        let outcome = architext_core::validate_data_dir(data_dir, schema_dir);
        if !outcome.ok {
            return Err(format!(
                "Rules update did not validate:\n{}",
                outcome.errors.join("\n")
            ));
        }
        Ok(())
    }
    .await;

    if let Err(msg) = write_result {
        write_set.restore().await;
        return Err(msg);
    }

    // Return { rules: [...], validation: { ok: true } }
    Ok(json!({
        "rules": next_document["rules"],
        "validation": { "ok": true }
    }))
}

fn apply_rules_action(document: &Value, payload: &Value) -> Result<Value, String> {
    let action = payload["action"].as_str().unwrap_or("update");
    match action {
        "update" => {
            let rule = &payload["rule"];
            upsert_rule(document, rule)
        }
        "delete" => {
            let id = payload["id"]
                .as_str()
                .ok_or_else(|| "delete action requires id".to_string())?;
            delete_rule(document, id)
        }
        "move" => {
            let id = payload["id"]
                .as_str()
                .ok_or_else(|| "move action requires id".to_string())?;
            let direction = payload["direction"]
                .as_str()
                .ok_or_else(|| "move action requires direction".to_string())?;
            move_rule(document, id, direction)
        }
        "move-before" => {
            let id = payload["id"]
                .as_str()
                .ok_or_else(|| "move-before action requires id".to_string())?;
            let before_id = payload["beforeId"]
                .as_str()
                .ok_or_else(|| "move-before action requires beforeId".to_string())?;
            move_rule_before(document, id, before_id)
        }
        other => Err(format!("Unknown rules action \"{other}\"")),
    }
}

/// HTTP 200 response with JSON body.
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

/// HTTP 200 with `{ok:false, mode, error, reload:false}` — matches JS error shape.
fn error_response(mode: &str, msg: &str) -> Response {
    ok_response(json!({
        "ok": false,
        "mode": mode,
        "error": msg,
        "reload": false
    }))
}
