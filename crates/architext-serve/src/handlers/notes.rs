//! Handler for `POST /api/notes`.
//!
//! Port of `updateNotesRequest` / `updateNotesRequestUnlocked` from
//! `src/adapters/http/notes-api.mjs`.
//!
//! Request body (JSON, max 1 MiB):
//!   `{ action?: "update"|"delete", ...action-specific fields }`
//!
//! Success response (HTTP 200):
//!   `{ notes: [...], validation: { ok: true, ... } }`
//!
//! Failure response (HTTP 200):
//!   `{ ok: false, mode: "notes", error: "<message>", reload: false }`
//!
//! Notes are optional: the first write self-bootstraps by registering
//! `manifest.files.notes = "notes.json"` and creating the file.  This
//! replicates the JS `manifestNeedsNotesEntry` logic exactly.

use axum::{
    body::Bytes,
    response::Response,
    Extension,
};
use serde_json::{json, Value};

use architext_core::domain::notes::{delete_note, upsert_note};
use architext_core::json_write::write_json_string;

use crate::handlers::rules::MAX_REQUEST_BODY_BYTES;
use crate::write_txn::WriteSet;
use crate::AppState;

const DEFAULT_NOTES_FILE: &str = "notes.json";

/// POST /api/notes
pub async fn post_notes(
    Extension(state): Extension<AppState>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response("notes", "Request body is too large");
    }

    let payload: Value = match serde_json::from_slice(if body.is_empty() { b"{}" } else { &body }) {
        Ok(v) => v,
        Err(e) => return error_response("notes", &format!("Invalid JSON: {e}")),
    };

    let lock = state.write_lock.lock().await;
    let result = update_notes_unlocked(&state, &payload).await;
    drop(lock);

    match result {
        Ok(body) => ok_response(body),
        Err(msg) => error_response("notes", &msg),
    }
}

async fn update_notes_unlocked(state: &AppState, payload: &Value) -> Result<Value, String> {
    let data_dir = &state.data_dir;

    // Read manifest
    let manifest_path = data_dir.join("manifest.json");
    let manifest_text = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| format!("Could not read manifest.json: {e}"))?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("Could not parse manifest.json: {e}"))?;

    // Self-bootstrap: notes are optional — use existing path or default.
    let notes_rel = manifest["files"]["notes"]
        .as_str()
        .unwrap_or(DEFAULT_NOTES_FILE)
        .to_string();
    let manifest_needs_notes_entry = manifest["files"]["notes"].as_str().is_none();
    let notes_path = data_dir.join(&notes_rel);

    // Read notes document, or start with empty if not present (ENOENT).
    let notes_document: Value = match tokio::fs::read_to_string(&notes_path).await {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("Could not parse notes file: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({ "notes": [] }),
        Err(e) => return Err(format!("Could not read notes file: {e}")),
    };

    // Apply domain action
    let next_document = apply_notes_action(&notes_document, payload)?;

    // Transactional write-set
    let mut write_set = WriteSet::new();

    let write_result: Result<(), String> = async {
        write_set
            .write(&notes_path, &write_json_string(&next_document))
            .await
            .map_err(|e| format!("Could not write notes file: {e}"))?;

        // If manifest did not have notes entry, register it now (in write-set
        // so it rolls back with the notes file on failure).
        if manifest_needs_notes_entry {
            let mut new_manifest = manifest.clone();
            let files = new_manifest["files"]
                .as_object_mut()
                .ok_or_else(|| "manifest.files is not an object".to_string())?;
            files.insert("notes".to_string(), Value::String(notes_rel.clone()));
            write_set
                .write(&manifest_path, &write_json_string(&new_manifest))
                .await
                .map_err(|e| format!("Could not write manifest.json: {e}"))?;
        }

        let schema_dir = state.schema_dir.as_path();
        let outcome = architext_core::validate_data_dir(data_dir, schema_dir);
        if !outcome.ok {
            return Err(format!(
                "Notes update did not validate:\n{}",
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

    Ok(json!({
        "notes": next_document["notes"],
        "validation": { "ok": true }
    }))
}

fn apply_notes_action(document: &Value, payload: &Value) -> Result<Value, String> {
    let action = payload["action"].as_str().unwrap_or("update");
    match action {
        "update" => {
            let note = &payload["note"];
            upsert_note(document, note)
        }
        "delete" => {
            let id = payload["id"]
                .as_str()
                .ok_or_else(|| "delete action requires id".to_string())?;
            delete_note(document, id)
        }
        other => Err(format!("Unknown notes action \"{other}\"")),
    }
}

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
