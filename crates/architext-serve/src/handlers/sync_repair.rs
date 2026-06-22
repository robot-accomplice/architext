//! Handler for `POST /api/sync-repair`.
//!
//! Port of `syncRepairApiRequest(target, version)` from
//! `src/adapters/cli/architext-cli.mjs` (~line 1299).
//!
//! This is a non-interactive sync+validate that applies doctor repairs and
//! validates. It mirrors the JS `syncTarget(target, { quiet: true, branch:
//! "none", noAgents: true, noGitignore: true })` call.
//!
//! Since the Rust serve layer doesn't have the full `syncTarget` pipeline
//! (no branch handling, no gitignore writes, no instruction
//! file upsert), the serve-layer `syncRepairApiRequest` is equivalent to:
//!   1. `collect_status` with validation.
//!   2. If there are doctor repairs AND installed && !needsMigration: apply them.
//!   3. Validate.
//!   4. Return envelope.
//!
//! This is the same as `doctorApiRequest` with `apply: true`, but the
//! response envelope shape is different (no `status` field, but has `output`).
//!
//! Response (HTTP 200):
//!   ```json
//!   { "ok": <validation.ok>, "output": "<log lines>",
//!     "validation": {...}, "reload": <validation.ok> }
//!   ```

use axum::{body::Bytes, response::Response, Extension};
use serde_json::{json, Value};

use architext_core::domain::doctor_repairs::apply_doctor_repairs;
use architext_core::status::collect_status;

use crate::handlers::rules::MAX_REQUEST_BODY_BYTES;
use crate::AppState;

/// POST /api/sync-repair
pub async fn post_sync_repair(
    Extension(state): Extension<AppState>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response("sync-repair", "Request body is too large");
    }

    // Body is ignored (no payload fields used)

    let cli_version = state.cli_version.as_ref().clone();
    let target_dir = state.target_dir.clone();
    let schema_dir = state.schema_dir.clone();

    let lock = state.write_lock.lock().await;

    let result = tokio::task::spawn_blocking(move || {
        let mut lines: Vec<String> = Vec::new();

        lines.push(format!("Target: {}", target_dir.display()));
        lines.push(format!("Architext CLI: {cli_version}"));

        let status = collect_status(&target_dir, &cli_version, true);
        let installed = status["installed"].as_bool().unwrap_or(false);
        let needs_migration = status["needsMigration"].as_bool().unwrap_or(false);

        if installed && !needs_migration {
            let repair_count = status["doctorRepairs"].as_array().map(|a| a.len()).unwrap_or(0);
            if repair_count > 0 {
                let applied = apply_doctor_repairs(&target_dir, &status, false, true);
                if !applied.is_empty() {
                    lines.push("Applied doctor repairs:".to_string());
                    for r in &applied {
                        lines.push(format!("- {}: {}", r.file, r.summary));
                    }
                }
            } else {
                lines.push("No doctor repairs needed.".to_string());
            }
        } else {
            lines.push("Target not installed or needs migration; skipping doctor repairs.".to_string());
        }

        let data_dir = target_dir.join("docs").join("architext").join("data");
        let outcome = architext_core::validate_data_dir(&data_dir, &schema_dir);
        let ok = outcome.ok;
        if ok {
            lines.push("Validation: passed".to_string());
        } else {
            lines.push("Validation: failed".to_string());
            for err in &outcome.errors {
                lines.push(format!("  - {err}"));
            }
        }

        let validation = json!({ "ok": ok });
        (ok, lines.join("\n"), validation)
    })
    .await
    .unwrap_or_else(|_| (false, "Internal error during sync-repair".to_string(), json!({ "ok": false })));

    drop(lock);

    let (ok, output, validation) = result;
    ok_response(json!({
        "ok": ok,
        "output": output,
        "validation": validation,
        "reload": ok
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
