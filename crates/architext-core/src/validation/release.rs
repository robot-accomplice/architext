//! Release and roadmap-target validation.
//!
//! Ports `validateReleaseReferences` from
//! `src/domain/architecture-model/references.mjs` and
//! `validateRoadmapReleaseTargets` from `viewer/tools/validate-architext.mjs`.
//!
//! Error strings are reproduced verbatim — the product's deep-validate depends
//! on them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Value};

// Re-use pure domain functions — single source of truth.
use crate::domain::release::{
    release_items as release_items_domain,
    release_summary_from_detail as release_summary_from_detail_domain,
};

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Load release files from the data directory, run reference checks, and
/// append any errors to `errors`.
///
/// Mirrors the `if (model.releases)` branch in `validate-architext.mjs main()`.
/// Returns the loaded releases (index + details) so the caller can run the
/// roadmap-target check.
pub fn validate_release_data(
    data_dir: &Path,
    releases_index_rel: &str,
    errors: &mut Vec<String>,
) -> Option<Releases> {
    let releases = load_releases(data_dir, releases_index_rel, errors)?;
    validate_release_references(&releases, errors);
    Some(releases)
}

/// Check that every roadmap item's `targetReleaseId` references a known
/// release id.
///
/// Mirrors `validateRoadmapReleaseTargets` in `validate-architext.mjs`.
pub fn validate_roadmap_release_targets(
    roadmap: &Value,
    releases: &Releases,
    errors: &mut Vec<String>,
) {
    let release_ids: HashSet<&str> = releases
        .index
        .get("releases")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|r| r.get("id").and_then(Value::as_str)).collect())
        .unwrap_or_default();

    let items = roadmap.get("items").and_then(Value::as_array);
    for item in items.into_iter().flatten() {
        let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
        if let Some(target) = item.get("targetReleaseId").and_then(Value::as_str) {
            if !release_ids.contains(target) {
                errors.push(format!(
                    "roadmap item {item_id}.targetReleaseId references unknown id \"{target}\""
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: load release data from disk
// ---------------------------------------------------------------------------

/// Loaded releases: the index document and each detail document.
pub struct Releases {
    pub index: Value,
    pub details: Vec<Value>,
}

fn load_releases(
    data_dir: &Path,
    index_rel: &str,
    errors: &mut Vec<String>,
) -> Option<Releases> {
    let index_path = data_dir.join(index_rel);
    let index = read_json(&index_path, errors)?;

    let detail_base = index_path.parent().unwrap_or(data_dir);
    let release_entries = index
        .get("releases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut details = Vec::new();
    for entry in &release_entries {
        if let Some(file) = entry.get("file").and_then(Value::as_str) {
            let detail_path = detail_base.join(file);
            if let Some(detail) = read_json(&detail_path, errors) {
                details.push(detail);
            }
        }
    }

    Some(Releases { index, details })
}

// ---------------------------------------------------------------------------
// Internal: reference checks (mirrors validateReleaseReferences in references.mjs)
// ---------------------------------------------------------------------------

fn validate_release_references(releases: &Releases, errors: &mut Vec<String>) {
    let index_releases = releases
        .index
        .get("releases")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    let release_ids: HashSet<&str> = index_releases
        .iter()
        .filter_map(|r| r.get("id").and_then(Value::as_str))
        .collect();

    // currentReleaseId must reference a known release
    if let Some(current_id) = releases.index.get("currentReleaseId").and_then(Value::as_str) {
        if !release_ids.contains(current_id) {
            errors.push(format!(
                "releases.currentReleaseId references unknown id \"{current_id}\""
            ));
        }
    }

    // Build a map of detail by id for O(1) lookup
    let details_by_id: HashMap<&str, &Value> = releases
        .details
        .iter()
        .filter_map(|d| d.get("id").and_then(Value::as_str).map(|id| (id, d)))
        .collect();

    // Per-summary checks
    for summary in index_releases {
        let id = match summary.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };

        let detail = match details_by_id.get(id) {
            Some(d) => d,
            None => continue, // requireAllDetails=false path; skip per-detail checks
        };

        // version must match
        if let (Some(sv), Some(dv)) = (
            summary.get("version").and_then(Value::as_str),
            detail.get("version").and_then(Value::as_str),
        ) {
            if sv != dv {
                errors.push(format!("release {id}.version does not match release index"));
            }
        }

        // status must match
        if let (Some(ss), Some(ds)) = (
            summary.get("status").and_then(Value::as_str),
            detail.get("status").and_then(Value::as_str),
        ) {
            if ss != ds {
                errors.push(format!("release {id}.status does not match release index"));
            }
        }

        // stale summary check
        let file = summary.get("file").and_then(Value::as_str).unwrap_or("");
        if !same_generated_release_summary(summary, detail, file) {
            errors.push(format!(
                "release {id}.index summary is stale; regenerate Release Truth history"
            ));
        }

        // completed requires releasedAt (on the index summary)
        let status = summary.get("status").and_then(Value::as_str).unwrap_or("");
        if status == "completed" && summary.get("releasedAt").and_then(Value::as_str).is_none() {
            errors.push(format!(
                "release index {id}.releasedAt is required for completed entries"
            ));
        }

        // implementing/planned/draft require targetDate or targetWindow
        if matches!(status, "implementing" | "planned" | "draft")
            && summary.get("targetDate").and_then(Value::as_str).is_none()
            && summary.get("targetWindow").and_then(Value::as_str).is_none()
        {
            errors.push(format!("release index {id} requires targetDate or targetWindow"));
        }
    }

    // Per-detail checks
    for detail in &releases.details {
        let id = match detail.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };

        let status = detail.get("status").and_then(Value::as_str).unwrap_or("");

        if status == "completed" && detail.get("releasedAt").and_then(Value::as_str).is_none() {
            errors.push(format!(
                "release {id}.releasedAt is required for completed entries"
            ));
        }
        if matches!(status, "implementing" | "planned" | "draft")
            && detail.get("targetDate").and_then(Value::as_str).is_none()
            && detail.get("targetWindow").and_then(Value::as_str).is_none()
        {
            errors.push(format!("release {id} requires targetDate or targetWindow"));
        }

        // Collect all scope items
        let items = release_items_domain(detail);
        let item_ids: HashSet<&str> = items
            .iter()
            .filter_map(|i| i.get("id").and_then(Value::as_str))
            .collect();
        let items_by_id: HashMap<&str, &Value> = items
            .iter()
            .filter_map(|i| i.get("id").and_then(Value::as_str).map(|id| (id, *i)))
            .collect();

        let workstreams = detail
            .get("workstreams")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        let workstream_ids: HashSet<&str> = workstreams
            .iter()
            .filter_map(|w| w.get("id").and_then(Value::as_str))
            .collect();

        // item.workstreamId and item.dependsOn
        for item in &items {
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
            if let Some(ws_id) = item.get("workstreamId").and_then(Value::as_str) {
                if !workstream_ids.contains(ws_id) {
                    errors.push(format!(
                        "release {id} item {item_id}.workstreamId references unknown id \"{ws_id}\""
                    ));
                }
            }
            if let Some(depends_on) = item.get("dependsOn").and_then(Value::as_array) {
                for dep in depends_on {
                    if let Some(dep_id) = dep.as_str() {
                        if !item_ids.contains(dep_id) {
                            errors.push(format!(
                                "release {id} item {item_id}.dependsOn references unknown id \"{dep_id}\""
                            ));
                        }
                    }
                }
            }
        }

        // workstream.itemIds
        for workstream in workstreams {
            let ws_id = workstream.get("id").and_then(Value::as_str).unwrap_or("");
            for item_id in arr_strs(workstream, "itemIds") {
                if !item_ids.contains(item_id) {
                    errors.push(format!(
                        "release {id} workstream {ws_id}.itemIds references unknown id \"{item_id}\""
                    ));
                }
            }
        }

        // blocker.itemIds + releaseItemCanBeBlocked guard
        let blockers = detail
            .get("blockers")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for blocker in blockers {
            let blocker_id = blocker.get("id").and_then(Value::as_str).unwrap_or("");
            for item_id in arr_strs(blocker, "itemIds") {
                if !item_ids.contains(item_id) {
                    errors.push(format!(
                        "release {id} blocker {blocker_id}.itemIds references unknown id \"{item_id}\""
                    ));
                }
                if let Some(item) = items_by_id.get(item_id) {
                    let item_status = item.get("status").and_then(Value::as_str).unwrap_or("");
                    if !release_item_can_be_blocked(item_status) {
                        errors.push(format!(
                            "release {id} blocker {blocker_id}.itemIds references {item_status} item \"{item_id}\""
                        ));
                    }
                }
            }
        }

        // milestone.itemIds
        let milestones = detail
            .get("milestones")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for milestone in milestones {
            let ms_id = milestone.get("id").and_then(Value::as_str).unwrap_or("");
            for item_id in arr_strs(milestone, "itemIds") {
                if !item_ids.contains(item_id) {
                    errors.push(format!(
                        "release {id} milestone {ms_id}.itemIds references unknown id \"{item_id}\""
                    ));
                }
            }
        }

        // dependency.from and .to
        let dependencies = detail
            .get("dependencies")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for dep in dependencies {
            let dep_id = dep.get("id").and_then(Value::as_str).unwrap_or("");
            if let Some(from) = dep.get("from").and_then(Value::as_str) {
                if !item_ids.contains(from) {
                    errors.push(format!(
                        "release {id} dependency {dep_id}.from references unknown id \"{from}\""
                    ));
                }
            }
            if let Some(to) = dep.get("to").and_then(Value::as_str) {
                if !item_ids.contains(to) {
                    errors.push(format!(
                        "release {id} dependency {dep_id}.to references unknown id \"{to}\""
                    ));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stale-summary check
// ---------------------------------------------------------------------------

/// Mirrors `sameGeneratedReleaseSummary(summary, detail)` in references.mjs.
///
/// Regenerates the index summary from the detail, normalizes both, and
/// compares their JSON serializations.
fn same_generated_release_summary(summary: &Value, detail: &Value, file: &str) -> bool {
    let generated = release_summary_from_detail_domain(detail, file);
    let norm_summary = normalize_release_summary(summary);
    let norm_generated = normalize_release_summary(&generated);
    serde_json::to_string(&norm_summary).ok() == serde_json::to_string(&norm_generated).ok()
}

/// Mirrors `normalizeReleaseSummary(summary)` in references.mjs.
///
/// Produces a JSON object with fields in the canonical order, null-coalescing
/// optional fields. The key order must match JS exactly for the
/// JSON.stringify comparison to work.
fn normalize_release_summary(summary: &Value) -> Value {
    json!({
        "id":           summary.get("id").and_then(Value::as_str).unwrap_or(""),
        "version":      summary.get("version").and_then(Value::as_str).unwrap_or(""),
        "name":         summary.get("name").and_then(Value::as_str).unwrap_or(""),
        "status":       summary.get("status").and_then(Value::as_str).unwrap_or(""),
        "posture":      summary.get("posture").and_then(Value::as_str).unwrap_or(""),
        "targetDate":   summary.get("targetDate").cloned().unwrap_or(Value::Null),
        "targetWindow": summary.get("targetWindow").cloned().unwrap_or(Value::Null),
        "releasedAt":   summary.get("releasedAt").cloned().unwrap_or(Value::Null),
        "lastUpdated":  summary.get("lastUpdated").and_then(Value::as_str).unwrap_or(""),
        "summary":      summary.get("summary").and_then(Value::as_str).unwrap_or(""),
        "counts":       summary.get("counts").cloned().unwrap_or(Value::Null),
        "file":         summary.get("file").and_then(Value::as_str).unwrap_or(""),
    })
}

/// Mirrors `releaseItemCanBeBlocked(status)` in references.mjs.
fn release_item_can_be_blocked(status: &str) -> bool {
    !matches!(status, "complete" | "deferred" | "cut")
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Iterate over string values in a JSON array field of an object.
fn arr_strs<'a>(obj: &'a Value, field: &str) -> impl Iterator<Item = &'a str> {
    obj.get(field)
        .and_then(Value::as_array)
        .map(|arr| arr.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| v.as_str())
}

/// Read a JSON file; return `None` and push an error on failure.
fn read_json(path: &Path, errors: &mut Vec<String>) -> Option<Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("cannot read {}: {}", path.display(), e));
            return None;
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(format!("invalid JSON in {}: {}", path.display(), e));
            None
        }
    }
}
