//! Handler for `POST /api/doctor`.
//!
//! Port of `doctorApiRequest(target, payload, version)` from
//! `src/adapters/cli/architext-cli.mjs` (~line 1255).
//!
//! Request body (JSON, max 1 MiB):
//!   `{ "apply": false }` — dry-run (default; no writes)
//!   `{ "apply": true }`  — apply detected repairs + re-validate
//!
//! Dry-run response (HTTP 200):
//!   ```json
//!   { "ok": true, "mode": "dry-run", "status": {...},
//!     "repairs": [...doctorRepairs], "validation": {...}, "reload": false }
//!   ```
//!
//! Apply response (HTTP 200):
//!   ```json
//!   { "ok": <validation.ok>, "mode": "apply", "status": {...},
//!     "repairs": [...applied], "validation": {...}, "reload": <validation.ok> }
//!   ```
//!
//! Apply blocked (not installed / needs migration):
//!   ```json
//!   { "ok": false, "mode": "apply", "status": {...}, "repairs": [],
//!     "validation": {...}, "reload": false,
//!     "output": "Run sync before doctor repairs." }
//!   ```
//!
//! `apply_doctor_repairs` lives in `architext_core::domain::doctor_repairs`
//! so the future CLI lifecycle port can also use it.

use axum::{body::Bytes, response::Response, Extension};
use serde_json::{json, Value};

use architext_core::domain::doctor_repairs::apply_doctor_repairs;
use architext_core::status::collect_status;

use crate::handlers::rules::MAX_REQUEST_BODY_BYTES;
use crate::AppState;

/// POST /api/doctor
pub async fn post_doctor(
    Extension(state): Extension<AppState>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response("doctor", "Request body is too large");
    }

    let payload: Value = match serde_json::from_slice(if body.is_empty() { b"{}" } else { &body }) {
        Ok(v) => v,
        Err(e) => return error_response("doctor", &format!("Invalid JSON: {e}")),
    };

    let cli_version = state.cli_version.as_ref().clone();

    // collect_status is synchronous I/O — run in blocking thread
    let target_dir = state.target_dir.clone();
    let version_clone = cli_version.clone();
    let status = tokio::task::spawn_blocking(move || {
        collect_status(&target_dir, &version_clone, true)
    })
    .await
    .unwrap_or_else(|_| json!({ "installed": false, "validation": { "ok": false }, "doctorRepairs": [] }));

    let apply = payload["apply"].as_bool().unwrap_or(false);

    if !apply {
        // Dry-run
        let repairs = status["doctorRepairs"].clone();
        let validation = status["validation"].clone();
        return ok_response(json!({
            "ok": true,
            "mode": "dry-run",
            "status": status,
            "repairs": repairs,
            "validation": validation,
            "reload": false
        }));
    }

    // Check: installed && !needsMigration
    let installed = status["installed"].as_bool().unwrap_or(false);
    let needs_migration = status["needsMigration"].as_bool().unwrap_or(false);
    if !installed || needs_migration {
        let validation = status["validation"].clone();
        return ok_response(json!({
            "ok": false,
            "mode": "apply",
            "status": status,
            "repairs": [],
            "validation": validation,
            "reload": false,
            "output": "Run sync before doctor repairs."
        }));
    }

    // Acquire write-lock and apply
    let lock = state.write_lock.lock().await;
    let target_dir2 = state.target_dir.clone();
    let schema_dir2 = state.schema_dir.clone();
    let version_clone2 = cli_version.clone();

    let result = tokio::task::spawn_blocking(move || {
        // Re-collect status under lock (mirrors JS re-collect inside withTargetWriteLock)
        let locked_status = collect_status(&target_dir2, &version_clone2, true);
        let locked_installed = locked_status["installed"].as_bool().unwrap_or(false);
        let locked_needs_migration = locked_status["needsMigration"].as_bool().unwrap_or(false);
        if !locked_installed || locked_needs_migration {
            return Err("Run sync before doctor repairs.".to_string());
        }

        let doctor_repairs_arr = locked_status["doctorRepairs"].as_array().cloned().unwrap_or_default();
        let repairs: Vec<Value> = if !doctor_repairs_arr.is_empty() {
            let applied = apply_doctor_repairs(&target_dir2, &locked_status, false, false);
            applied.iter().map(|r| r.to_json()).collect()
        } else {
            vec![]
        };

        let outcome = architext_core::validate_data_dir(&target_dir2.join("docs").join("architext").join("data"), &schema_dir2);
        let validation = json!({ "ok": outcome.ok });
        Ok((repairs, validation))
    })
    .await
    .unwrap_or_else(|_| Err("Internal error during doctor repairs".to_string()));

    drop(lock);

    match result {
        Err(msg) => {
            let validation = status["validation"].clone();
            ok_response(json!({
                "ok": false,
                "mode": "apply",
                "status": status,
                "repairs": [],
                "validation": validation,
                "reload": false,
                "output": msg
            }))
        }
        Ok((repairs, validation)) => {
            let ok = validation["ok"].as_bool().unwrap_or(false);
            ok_response(json!({
                "ok": ok,
                "mode": "apply",
                "status": status,
                "repairs": repairs,
                "validation": validation,
                "reload": ok
            }))
        }
    }
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
