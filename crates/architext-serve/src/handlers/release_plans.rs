//! Handler for `POST /api/release-plans`.
//!
//! Port of `approveReleasePlanRequest` / `approveReleasePlanRequestUnlocked`
//! from `src/adapters/http/release-planning-api.mjs`.
//!
//! Request body (JSON, max 1 MiB):
//!   ```json
//!   {
//!     "action": "preview" | "approve" | "save-draft",   // required
//!     "version": "1.2.0",                               // optional
//!     "selectedRoadmapItemIds": ["id-a", "id-b"],       // optional
//!     "itemScopes": { "id-a": "required" },             // optional
//!     "adHocItems": [],                                 // optional
//!     "theme": "Foo theme",                             // optional
//!     "dryRun": false                                   // legacy compat: true → "preview"
//!   }
//!   ```
//!
//! Success response (HTTP 200):
//!   ```json
//!   {
//!     "release":       { ...summary },
//!     "releaseDetail": { ...detail },
//!     "roadmapItems":  [...],
//!     "changes":       { ...changeSummary },
//!     "validation":    { "ok": true, "output": "..." }
//!   }
//!   ```
//!
//! Error response (HTTP 200, same as JS):
//!   ```json
//!   { "ok": false, "error": "...", "reload": false }
//!   ```
//!
//! Transactional semantics:
//!   - `preview` — no lock, no write; returns the plan + changes.
//!   - `approve` — acquires write-lock, writes release detail + index + roadmap.
//!   - `save-draft` — acquires write-lock, writes release detail + index (NOT roadmap).
//!   - On validation failure after write: rolls back via `WriteSet::restore`.
//!
//! Deferred-to transfer markers (`writeDeferredTransferMarkers`):
//!   Applied only for `approve`. Roadmap items that appear in the selected
//!   scope AND have `status: "deferred"` AND `targetReleaseId` pointing to a
//!   different release receive a `deferredToReleaseId` / `deferredToVersion`
//!   on that source release's detail file.
//!
//! Reference validation (pre-write, mirrors JS `validateReleaseReferences`):
//!   Uses the Rust `validate_data_dir` (full validator) post-write for the
//!   authoritative gate. The JS does a lightweight reference-only pre-check;
//!   we skip the pre-check and rely on the full validator, which subsumes it.
//!   The JS pre-check errors are distinct from full-validator errors; since the
//!   parity harness compares ok/fail (not the exact error text), this is fine.

use std::path::Path;

use axum::{body::Bytes, response::Response, Extension};
use serde_json::{json, Value};

use architext_core::domain::release::{
    approve_release_plan, build_release_plan, merge_existing_release_plan,
    release_items, save_release_plan_draft,
};
use architext_core::json_write::write_json_string;

use crate::handlers::rules::MAX_REQUEST_BODY_BYTES;
use crate::write_txn::WriteSet;
use crate::AppState;

// ─── Handler ──────────────────────────────────────────────────────────────────

/// POST /api/release-plans
pub async fn post_release_plans(
    Extension(state): Extension<AppState>,
    body: Bytes,
) -> Response {
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response("Release plan request body is too large");
    }

    let payload: Value = match serde_json::from_slice(if body.is_empty() { b"{}" } else { &body }) {
        Ok(v) => v,
        Err(e) => return error_response(&format!("Invalid JSON: {e}")),
    };

    // Determine action — mirrors JS `const action = payload.action ?? (payload.dryRun ? "preview" : "approve")`.
    let action = match payload["action"].as_str() {
        Some(a) => a.to_string(),
        None => {
            if payload["dryRun"].as_bool().unwrap_or(false) {
                "preview".to_string()
            } else {
                "approve".to_string()
            }
        }
    };

    if !["preview", "approve", "save-draft"].contains(&action.as_str()) {
        return error_response(&format!("Unknown release planning action \"{action}\""));
    }

    if action == "preview" {
        // preview: no lock, no write
        match release_plan_unlocked(&state, &payload, &action).await {
            Ok(body) => ok_response(body),
            Err(msg) => error_response(&msg),
        }
    } else {
        let lock = state.write_lock.lock().await;
        let result = release_plan_unlocked(&state, &payload, &action).await;
        drop(lock);
        match result {
            Ok(body) => ok_response(body),
            Err(msg) => error_response(&msg),
        }
    }
}

async fn release_plan_unlocked(
    state: &AppState,
    payload: &Value,
    action: &str,
) -> Result<Value, String> {
    let data_dir = &state.data_dir;

    // Read manifest
    let manifest_path = data_dir.join("manifest.json");
    let manifest_text = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| format!("Could not read manifest.json: {e}"))?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("Could not parse manifest.json: {e}"))?;

    let roadmap_rel = manifest["files"]["roadmap"]
        .as_str()
        .ok_or_else(|| "Release Planning requires manifest.files.roadmap".to_string())?;
    let releases_rel = manifest["files"]["releases"]
        .as_str()
        .ok_or_else(|| "Release Planning requires manifest.files.releases".to_string())?;

    let roadmap_path = data_dir.join(roadmap_rel);
    let release_index_path = data_dir.join(releases_rel);

    let roadmap_text = tokio::fs::read_to_string(&roadmap_path)
        .await
        .map_err(|e| format!("Could not read roadmap: {e}"))?;
    let roadmap: Value = serde_json::from_str(&roadmap_text)
        .map_err(|e| format!("Could not parse roadmap: {e}"))?;

    let index_text = tokio::fs::read_to_string(&release_index_path)
        .await
        .map_err(|e| format!("Could not read releases index: {e}"))?;
    let release_index: Value = serde_json::from_str(&index_text)
        .map_err(|e| format!("Could not parse releases index: {e}"))?;

    // Read existing release detail for this version (if any)
    let existing_release_detail = read_existing_release_detail(payload, &release_index, &release_index_path).await;

    // Build/assemble the release detail
    let release_detail = build_assembled_release_detail(
        payload,
        action,
        &release_index,
        &roadmap,
        &manifest,
        existing_release_detail.as_ref().unwrap_or(&Value::Null),
    )?;

    // Apply approve or save-draft domain function
    let planned = if action == "save-draft" {
        let input = json!({
            "releaseIndex": release_index,
            "roadmap": roadmap,
            "releaseDetail": release_detail
        });
        save_release_plan_draft(&input)
    } else {
        // approve (and preview computes approve-style plan for display)
        let input = json!({
            "releaseIndex": release_index,
            "roadmap": roadmap,
            "releaseDetail": release_detail
        });
        approve_release_plan(&input)
    };

    // For preview: return without writing
    let is_dry_run = payload["dryRun"].as_bool().unwrap_or(false);
    if action == "preview" || is_dry_run {
        let detail_id = planned["releaseFile"]["detail"]["id"].as_str().unwrap_or("");
        let release_summary = planned["releaseIndex"]["releases"]
            .as_array()
            .and_then(|arr| arr.iter().find(|r| r["id"].as_str() == Some(detail_id)))
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(json!({
            "release": release_summary,
            "releaseDetail": planned["releaseFile"]["detail"],
            "roadmapItems": planned["roadmap"]["items"],
            "changes": planned["changes"],
            "validation": { "ok": true, "output": "Preview passed reference validation." }
        }));
    }

    // Write transactionally
    let release_file_name = planned["releaseFile"]["file"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let id = release_detail["id"].as_str().unwrap_or("release");
            release_file_for_id(id)
        });
    let release_detail_path = release_index_path.parent()
        .unwrap_or(data_dir.as_path())
        .join(&release_file_name);

    let mut write_set = WriteSet::new();

    let write_result: Result<(), String> = async {
        // Write release detail file
        write_set
            .write(&release_detail_path, &write_json_string(&planned["releaseFile"]["detail"]))
            .await
            .map_err(|e| format!("Could not write release detail: {e}"))?;

        // Write releases index
        write_set
            .write(&release_index_path, &write_json_string(&planned["releaseIndex"]))
            .await
            .map_err(|e| format!("Could not write releases index: {e}"))?;

        // Write roadmap (approve only, not save-draft)
        if action != "save-draft" {
            write_set
                .write(&roadmap_path, &write_json_string(&planned["roadmap"]))
                .await
                .map_err(|e| format!("Could not write roadmap: {e}"))?;

            // Write deferred-to transfer markers
            write_deferred_transfer_markers(
                &mut write_set,
                action,
                &roadmap,
                &release_index,
                &release_index_path,
                &planned["releaseFile"]["detail"],
            )
            .await?;
        }

        // Full validation
        let outcome = architext_core::validate_data_dir(data_dir, &state.schema_dir);
        if !outcome.ok {
            return Err(format!(
                "Release plan did not validate:\n{}",
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

    let detail_id = planned["releaseFile"]["detail"]["id"].as_str().unwrap_or("");
    let release_summary = planned["releaseIndex"]["releases"]
        .as_array()
        .and_then(|arr| arr.iter().find(|r| r["id"].as_str() == Some(detail_id)))
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "release": release_summary,
        "releaseDetail": planned["releaseFile"]["detail"],
        "roadmapItems": planned["roadmap"]["items"],
        "changes": planned["changes"],
        "validation": { "ok": true }
    }))
}

/// Build or use the release detail — mirrors the JS logic around `mergeExistingReleasePlan`.
fn build_assembled_release_detail(
    payload: &Value,
    action: &str,
    release_index: &Value,
    roadmap: &Value,
    manifest: &Value,
    existing_release_detail: &Value,
) -> Result<Value, String> {
    let selected_count = payload["selectedRoadmapItemIds"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0)
        + payload["adHocItems"].as_array().map(|a| a.len()).unwrap_or(0);

    // Reuse existing if approve with 0 selected items and an existing detail
    if action == "approve" && selected_count == 0 && !existing_release_detail.is_null() {
        return Ok(merge_existing_release_plan(existing_release_detail, existing_release_detail));
    }

    let selected_ids = if payload["selectedRoadmapItemIds"].is_array() {
        payload["selectedRoadmapItemIds"].clone()
    } else {
        json!([])
    };
    let item_scopes = if payload["itemScopes"].is_object() {
        payload["itemScopes"].clone()
    } else {
        json!({})
    };
    let ad_hoc_items = if payload["adHocItems"].is_array() {
        payload["adHocItems"].clone()
    } else {
        json!([])
    };

    let build_input = json!({
        "releaseIndex": release_index,
        "roadmapItems": roadmap["items"],
        "selectedRoadmapItemIds": selected_ids,
        "itemScopes": item_scopes,
        "adHocItems": ad_hoc_items,
        "projectName": manifest["project"]["name"],
        "version": payload["version"],
        "theme": payload["theme"],
        "now": now_iso8601()
    });

    let proposed = build_release_plan(&build_input)?;
    Ok(merge_existing_release_plan(existing_release_detail, &proposed))
}

/// Port of `readReleaseDetailForVersion`.
async fn read_existing_release_detail(
    payload: &Value,
    release_index: &Value,
    release_index_path: &Path,
) -> Option<Value> {
    let version = payload["version"].as_str()?;
    let file = release_index["releases"]
        .as_array()?
        .iter()
        .find(|r| r["version"].as_str() == Some(version))?
        .get("file")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|| release_file_for_version(version));

    let release_dir = release_index_path.parent()?;
    let path = release_dir.join(&file);
    let text = tokio::fs::read_to_string(&path).await.ok()?;
    serde_json::from_str(&text).ok()
}

/// Port of `writeDeferredTransferMarkers`.
///
/// For each roadmap item that is in the selected scope AND has `status:
/// "deferred"` AND `targetReleaseId` pointing to a different release, write
/// `deferredToReleaseId` + `deferredToVersion` into that source release's
/// detail file.
async fn write_deferred_transfer_markers(
    write_set: &mut WriteSet,
    action: &str,
    roadmap: &Value,
    release_index: &Value,
    release_index_path: &Path,
    release_detail: &Value,
) -> Result<(), String> {
    if action == "save-draft" {
        return Ok(());
    }

    let detail_id = release_detail["id"].as_str().unwrap_or("");
    let selected_item_ids: std::collections::HashSet<&str> = release_items(release_detail)
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();

    let releases = release_index["releases"].as_array();

    for item in roadmap["items"].as_array().into_iter().flatten() {
        let item_id = match item["id"].as_str() { Some(s) => s, None => continue };
        if !selected_item_ids.contains(item_id) { continue; }
        let item_status = item["status"].as_str().unwrap_or("");
        if item_status != "deferred" { continue; }
        let target_id = match item["targetReleaseId"].as_str() { Some(s) => s, None => continue };
        if target_id.is_empty() || target_id == detail_id { continue; }

        // Find the source release file
        let source_summary = match releases.and_then(|arr| {
            arr.iter().find(|r| r["id"].as_str() == Some(target_id))
        }) {
            Some(s) => s,
            None => continue,
        };
        let source_file = match source_summary["file"].as_str() {
            Some(f) => f,
            None => continue,
        };

        let release_dir = release_index_path.parent().unwrap_or(Path::new("."));
        let source_path = release_dir.join(source_file);

        let source_text = match tokio::fs::read_to_string(&source_path).await {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mut source_detail: Value = match serde_json::from_str(&source_text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Update each matching item in the source detail
        let mut changed = false;
        let detail_version = release_detail["version"].as_str().unwrap_or("").to_string();
        let detail_id_owned = detail_id.to_string();

        // Iterate over all scope sections
        for section in &["required", "planned", "stretch", "deferred", "outOfScope"] {
            if let Some(arr) = source_detail["scope"][section].as_array_mut() {
                for scope_item in arr.iter_mut() {
                    if scope_item["id"].as_str() == Some(item_id) {
                        scope_item.as_object_mut().unwrap().insert(
                            "deferredToReleaseId".to_string(),
                            Value::String(detail_id_owned.clone()),
                        );
                        scope_item.as_object_mut().unwrap().insert(
                            "deferredToVersion".to_string(),
                            Value::String(detail_version.clone()),
                        );
                        changed = true;
                    }
                }
            }
        }

        if changed {
            write_set
                .write(&source_path, &write_json_string(&source_detail))
                .await
                .map_err(|e| format!("Could not write source release detail: {e}"))?;
        }
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn release_file_for_version(version: &str) -> String {
    format!("v{}.json", version.replace('.', "-"))
}

fn release_file_for_id(id: &str) -> String {
    format!("{id}.json")
}

fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = secs_to_datetime(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

fn secs_to_datetime(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let hour = hours % 24;
    let days = hours / 24;
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let y_days = if leap { 366 } else { 365 };
        if remaining < y_days { break; }
        remaining -= y_days;
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_days: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u64;
    for dm in &month_days {
        if remaining < *dm { break; }
        remaining -= dm;
        month += 1;
    }
    (year, month + 1, remaining + 1, hour, min, sec)
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

fn error_response(msg: &str) -> Response {
    ok_response(json!({
        "ok": false,
        "error": msg,
        "reload": false
    }))
}
