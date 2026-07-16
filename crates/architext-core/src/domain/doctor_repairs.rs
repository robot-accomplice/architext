//! `apply_doctor_repairs` — Rust port of `applyDoctorRepairs` from
//! `src/adapters/cli/architext-cli.mjs` (~line 798).
//!
//! This module is in `architext-core` (not `architext-serve`) so that the
//! future CLI lifecycle port can also compose it without duplicating domain
//! logic. The serve handlers import it from here.
//!
//! All four repair categories are ported:
//!   - `c4`               → `repair_c4_views` (domain::c4_quality)
//!   - `manifest`         → bump schemaVersion to DATA_SCHEMA_VERSION
//!   - `release-truth`    → `generated_release_index` (domain::release)
//!   - `instruction-rules`→ `planned_instruction_rule_migration` (domain::instruction_rules)
//!
//! I/O lives here (fs reads + `write_json_string`). The output is a Vec of
//! `DoctorRepair` records, one per change applied. Mirrors the JS return shape.

use std::path::Path;

use serde_json::{json, Value};

use crate::domain::{c4_quality, instruction_rules, release, schema_migration};
use crate::json_write::write_json_string;

pub const DATA_SCHEMA_VERSION: &str = "1.5.0";

/// A single repair action applied (mirrors JS `{ category, file, summary }`).
#[derive(Debug, Clone)]
pub struct DoctorRepair {
    pub category: String,
    pub file: String,
    pub summary: String,
}

impl DoctorRepair {
    pub fn to_json(&self) -> Value {
        json!({
            "category": self.category,
            "file": self.file,
            "summary": self.summary
        })
    }
}

// ─── Path helpers (mirrors JS `dataDir(target)`) ─────────────────────────────

fn data_dir(target: &Path) -> std::path::PathBuf {
    target.join("docs").join("architext").join("data")
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(path, write_json_string(value).as_bytes())
}

// ─── repairC4Data ─────────────────────────────────────────────────────────────

/// Port of `repairC4Data(target, dryRun)`.
pub fn repair_c4_data(target: &Path, dry_run: bool) -> Vec<String> {
    let target_data_dir = data_dir(target);
    let views_path = target_data_dir.join("views.json");
    let nodes_path = target_data_dir.join("nodes.json");

    if !views_path.exists() || !nodes_path.exists() {
        return vec![];
    }

    let views_doc = match read_json(&views_path) {
        Some(v) => v,
        None => return vec![],
    };
    let nodes_doc = match read_json(&nodes_path) {
        Some(v) => v,
        None => return vec![],
    };

    let nodes_arr = nodes_doc["nodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let node_map = c4_quality::build_node_map(nodes_arr);
    let views_arr = views_doc["views"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let repaired = c4_quality::repair_c4_views(views_arr, &node_map);
    let changes: Vec<String> = repaired["changes"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    if !changes.is_empty() && !dry_run {
        let mut new_doc = views_doc.clone();
        new_doc.as_object_mut().unwrap().insert(
            "views".to_string(),
            repaired["views"].clone(),
        );
        let _ = write_json(&views_path, &new_doc);
    }

    changes
}

// ─── repairManifestData ───────────────────────────────────────────────────────

/// Port of `repairManifestData(target, dryRun)`.
pub fn repair_manifest_data(target: &Path, dry_run: bool) -> Vec<String> {
    let manifest_path = data_dir(target).join("manifest.json");
    if !manifest_path.exists() {
        return vec![];
    }
    let manifest = match read_json(&manifest_path) {
        Some(v) => v,
        None => return vec![],
    };

    let current = manifest["schemaVersion"].as_str().unwrap_or("");
    let migration_plan = schema_migration::schema_migration_plan(current, DATA_SCHEMA_VERSION);
    let repair_changes: Vec<String> = migration_plan["pending"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["summary"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if !repair_changes.is_empty() && !dry_run {
        let mut new_manifest = manifest.clone();
        new_manifest
            .as_object_mut()
            .unwrap()
            .insert("schemaVersion".to_string(), Value::String(DATA_SCHEMA_VERSION.to_string()));
        let _ = write_json(&manifest_path, &new_manifest);
    }

    repair_changes
}

// ─── repairReleaseTruthData ───────────────────────────────────────────────────

/// Port of `repairReleaseTruthData(target, dryRun)`.
pub fn repair_release_truth_data(target: &Path, dry_run: bool) -> Vec<String> {
    let target_data_dir = data_dir(target);
    let manifest_path = target_data_dir.join("manifest.json");
    if !manifest_path.exists() {
        return vec![];
    }
    let manifest = match read_json(&manifest_path) {
        Some(v) => v,
        None => return vec![],
    };

    if let Some(releases_rel) = manifest["files"]["releases"].as_str() {
        let index_path = target_data_dir.join(releases_rel);
        let release_dir = index_path.parent().unwrap_or(&index_path);
        let index = match read_json(&index_path) {
            Some(v) => v,
            // Missing or unparseable index: regenerate it from the detail files on
            // disk. Status advertises this case as "create missing Release Truth
            // history index" (generatedReleaseHistoryChanges), so the applied
            // change summary must match.
            None => {
                let detail_entries = release_detail_entries_from_dir(release_dir, &index_path);
                let generated = release::generated_release_index(&Value::Null, &detail_entries);
                if !dry_run {
                    let _ = std::fs::create_dir_all(release_dir);
                    let _ = write_json(&index_path, &generated);
                }
                return vec!["create missing Release Truth history index".to_string()];
            }
        };
        let detail_entries = build_release_detail_entries(release_dir, &index);
        let generated = release::generated_release_index(&index, &detail_entries);
        let changes_val = release::release_index_generation_changes(&index, &generated);
        let repair_changes: Vec<String> = changes_val
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        if !repair_changes.is_empty() && !dry_run {
            let _ = write_json(&index_path, &generated);
        }
        return repair_changes;
    }

    // manifest.files.releases absent — write starter data
    let repair_changes = vec!["add starter Release Truth data and manifest.files.releases".to_string()];
    if !dry_run {
        let releases_dir = target_data_dir.join("releases");
        let _ = std::fs::create_dir_all(&releases_dir);

        let mut new_manifest = manifest.clone();
        new_manifest["files"]
            .as_object_mut()
            .unwrap()
            .insert("releases".to_string(), Value::String("releases/index.json".to_string()));
        let _ = write_json(&manifest_path, &new_manifest);

        write_starter_release_data(&releases_dir);
    }
    repair_changes
}

fn write_starter_release_data(releases_dir: &Path) {
    let release_id = "initial-architext-buildout";
    let last_updated = chrono_now();

    let index = json!({
        "currentReleaseId": release_id,
        "releases": [{
            "id": release_id,
            "version": "0.1.0",
            "name": "Initial Architext build-out",
            "status": "planned",
            "posture": "at-risk",
            "targetWindow": "Before claiming architecture documentation is current",
            "lastUpdated": last_updated,
            "summary": "Replace starter architecture and release data with project-specific facts.",
            "counts": {
                "features": 0,
                "bugFixes": 0,
                "workstreams": 1,
                "blockers": 1,
                "complete": 0,
                "inProgress": 0,
                "planned": 1,
                "stretch": 0
            },
            "file": format!("{release_id}.json")
        }]
    });
    let _ = write_json(&releases_dir.join("index.json"), &index);

    let detail = json!({
        "id": release_id,
        "version": "0.1.0",
        "name": "Initial Architext build-out",
        "status": "planned",
        "posture": "at-risk",
        "targetWindow": "Before claiming architecture documentation is current",
        "lastUpdated": last_updated,
        "summary": "Replace starter architecture and release data with project-specific facts.",
        "scope": {
            "required": [],
            "planned": [{
                "id": "populate-architecture-data",
                "kind": "workstream",
                "title": "Populate architecture data",
                "status": "planned",
                "workstreamId": "initial-buildout",
                "dateAdded": last_updated
            }],
            "stretch": [],
            "deferred": [],
            "outOfScope": []
        },
        "workstreams": [{
            "id": "initial-buildout",
            "name": "Initial build-out",
            "owner": "maintainer",
            "status": "planned",
            "posture": "on-track",
            "summary": "Bootstrap Architext architecture documentation.",
            "progress": 0,
            "itemIds": ["populate-architecture-data"],
            "evidence": []
        }],
        "blockers": [{
            "id": "architecture-data-absent",
            "title": "Architecture data absent",
            "status": "open",
            "summary": "Replace starter data with real architecture facts before claiming this documentation is current.",
            "dateAdded": last_updated
        }],
        "milestones": [],
        "dependencies": [],
        "evidence": []
    });
    let _ = write_json(&releases_dir.join(format!("{release_id}.json")), &detail);
}

fn chrono_now() -> String {
    // Use std time only; no chrono dep needed for a timestamp string
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Format as ISO 8601 — approximate (no sub-second, no tz offset awareness)
    let s = secs;
    let (y, mo, d, h, mi, sec) = secs_to_datetime(s);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}.000Z")
}

fn secs_to_datetime(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let hour = hours % 24;
    let days = hours / 24;

    // Simplified Gregorian calendar
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u64;
    for days_in_month in &month_days {
        if remaining < *days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }
    (year, month + 1, remaining + 1, hour, min, sec)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// ─── repairInstructionRules ───────────────────────────────────────────────────

/// Port of `repairInstructionRules(target, dryRun)`.
pub fn repair_instruction_rules(target: &Path, dry_run: bool) -> Vec<String> {
    let rules_path = data_dir(target).join("rules.json");
    if !rules_path.exists() {
        return vec![];
    }
    let rules_doc = match read_json(&rules_path) {
        Some(v) => v,
        None => return vec![],
    };

    let source_files = collect_instruction_rule_source_files(target);
    let existing_rules = rules_doc["rules"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let migration = instruction_rules::planned_instruction_rule_migration(&source_files, existing_rules);
    let repair_changes: Vec<String> = migration["repairChanges"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    if !repair_changes.is_empty() && !dry_run {
        // Add candidate rules
        if let Some(candidates) = migration["candidateRules"].as_array() {
            if !candidates.is_empty() {
                let mut new_doc = rules_doc.clone();
                let existing = new_doc["rules"].as_array_mut().unwrap();
                for c in candidates {
                    existing.push(c.clone());
                }
                let _ = write_json(&rules_path, &new_doc);
            }
        }
        // Rewrite instruction files
        if let Some(rewrites) = migration["rewriteFiles"].as_array() {
            for rewrite in rewrites {
                let path_str = match rewrite["path"].as_str() { Some(s) => s, None => continue };
                let replacement = match rewrite["replacement"].as_str() { Some(s) => s, None => continue };
                let full_path = target.join(path_str);
                let content = if replacement.ends_with('\n') {
                    replacement.to_string()
                } else {
                    format!("{replacement}\n")
                };
                let _ = std::fs::write(&full_path, content.as_bytes());
            }
        }
    }

    repair_changes
}

fn collect_instruction_rule_source_files(target: &Path) -> Vec<Value> {
    let explicit: Vec<std::path::PathBuf> = instruction_rules::INSTRUCTION_RULE_FILES
        .iter()
        .map(|name| target.join(name))
        .collect();

    // Cursor rules
    let cursor_rules_dir = target.join(".cursor").join("rules");
    let cursor_files: Vec<std::path::PathBuf> = if cursor_rules_dir.exists() {
        let re = regex::Regex::new(r"\.(md|mdc|txt)$").unwrap();
        std::fs::read_dir(&cursor_rules_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter(|e| re.is_match(e.file_name().to_string_lossy().as_ref()))
            .map(|e| e.path())
            .collect()
    } else {
        vec![]
    };

    let mut files = Vec::new();
    for abs_path in explicit.into_iter().chain(cursor_files) {
        if !abs_path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&abs_path).unwrap_or_default();
        let rel = abs_path
            .strip_prefix(target)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        files.push(json!({
            "path": rel,
            "absolutePath": abs_path.to_string_lossy(),
            "text": text
        }));
    }
    files
}

// ─── build_release_detail_entries ────────────────────────────────────────────

/// Discover release detail entries directly from the releases directory, for the
/// case where no readable index exists to enumerate them. Every `*.json` file
/// except the index itself is treated as a detail file; unreadable files are
/// skipped. Sorted by file name for deterministic output.
fn release_detail_entries_from_dir(release_dir: &Path, index_path: &Path) -> Value {
    let mut files: Vec<String> = std::fs::read_dir(release_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path == index_path || path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            path.file_name().and_then(|n| n.to_str()).map(|n| n.to_string())
        })
        .collect();
    files.sort();
    let entries: Vec<Value> = files
        .iter()
        .filter_map(|file| {
            let detail = read_json(&release_dir.join(file))?;
            Some(json!({ "file": file, "detail": detail }))
        })
        .collect();
    Value::Array(entries)
}

fn build_release_detail_entries(release_dir: &Path, index: &Value) -> Value {
    let releases = match index["releases"].as_array() {
        Some(arr) => arr,
        None => return json!([]),
    };
    let entries: Vec<Value> = releases
        .iter()
        .filter_map(|summary| {
            let file = summary["file"].as_str()?;
            let detail = read_json(&release_dir.join(file))?;
            Some(json!({ "file": file, "detail": detail }))
        })
        .collect();
    Value::Array(entries)
}

// ─── doctorRepairCategories ───────────────────────────────────────────────────

/// Port of `doctorRepairCategories(doctorRepairs)` — extracts unique categories in order.
pub fn doctor_repair_categories(doctor_repairs: &[Value]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut categories = Vec::new();
    for repair in doctor_repairs {
        if let Some(cat) = repair["category"].as_str() {
            if seen.insert(cat.to_string()) {
                categories.push(cat.to_string());
            }
        }
    }
    categories
}

// ─── Public: apply_doctor_repairs ────────────────────────────────────────────

/// Port of `applyDoctorRepairs(target, status, dryRun, { skipInstructionRules })`.
///
/// `status` is the `serde_json::Value` from `collect_status`.
/// Returns a `Vec<DoctorRepair>` — one entry per change applied (or planned if dry_run).
pub fn apply_doctor_repairs(
    target: &Path,
    status: &Value,
    dry_run: bool,
    skip_instruction_rules: bool,
) -> Vec<DoctorRepair> {
    let doctor_repairs = match status["doctorRepairs"].as_array() {
        Some(arr) => arr.as_slice().to_vec(),
        None => return vec![],
    };

    let categories = doctor_repair_categories(&doctor_repairs)
        .into_iter()
        .filter(|cat| !(skip_instruction_rules && cat == "instruction-rules"))
        .collect::<Vec<_>>();

    let repair_files = |cat: &str| -> String {
        match cat {
            "c4" => data_dir(target).join("views.json").to_string_lossy().to_string(),
            "manifest" => data_dir(target).join("manifest.json").to_string_lossy().to_string(),
            "release-truth" => data_dir(target).join("releases").join("index.json").to_string_lossy().to_string(),
            "instruction-rules" => data_dir(target).join("rules.json").to_string_lossy().to_string(),
            other => data_dir(target).join(other).to_string_lossy().to_string(),
        }
    };

    let mut applied = Vec::new();

    for category in &categories {
        let changes: Vec<String> = match category.as_str() {
            "c4" => repair_c4_data(target, dry_run),
            "manifest" => repair_manifest_data(target, dry_run),
            "release-truth" => repair_release_truth_data(target, dry_run),
            "instruction-rules" => repair_instruction_rules(target, dry_run),
            _ => vec![],
        };
        let file = repair_files(category);
        for summary in changes {
            applied.push(DoctorRepair {
                category: category.clone(),
                file: file.clone(),
                summary,
            });
        }
    }

    applied
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content.as_bytes()).unwrap();
    }

    #[test]
    fn repair_manifest_data_no_file() {
        let td = temp_dir();
        let changes = repair_manifest_data(td.path(), false);
        assert!(changes.is_empty());
    }

    #[test]
    fn repair_manifest_data_up_to_date() {
        let td = temp_dir();
        write(td.path(), "docs/architext/data/manifest.json",
            &format!("{{\"schemaVersion\":\"{}\",\"files\":{{}},\"project\":{{\"name\":\"test\"}}}}\n", DATA_SCHEMA_VERSION));
        let changes = repair_manifest_data(td.path(), false);
        assert!(changes.is_empty());
    }

    #[test]
    fn repair_manifest_data_outdated_dry_run() {
        let td = temp_dir();
        write(td.path(), "docs/architext/data/manifest.json",
            "{\"schemaVersion\":\"1.0.0\",\"files\":{},\"project\":{\"name\":\"test\"}}\n");
        let changes = repair_manifest_data(td.path(), true);
        assert!(!changes.is_empty());
        // dry run: file should still have old version
        let text = fs::read_to_string(td.path().join("docs/architext/data/manifest.json")).unwrap();
        assert!(text.contains("1.0.0"));
    }

    #[test]
    fn repair_manifest_data_outdated_applies() {
        let td = temp_dir();
        write(td.path(), "docs/architext/data/manifest.json",
            "{\"schemaVersion\":\"1.0.0\",\"files\":{},\"project\":{\"name\":\"test\"}}\n");
        let changes = repair_manifest_data(td.path(), false);
        assert!(!changes.is_empty());
        // file should now have the target version
        let text = fs::read_to_string(td.path().join("docs/architext/data/manifest.json")).unwrap();
        assert!(text.contains(DATA_SCHEMA_VERSION));
    }

    #[test]
    fn repair_c4_no_files() {
        let td = temp_dir();
        let changes = repair_c4_data(td.path(), false);
        assert!(changes.is_empty());
    }

    #[test]
    fn doctor_repair_categories_dedup_ordered() {
        let repairs = vec![
            json!({"category": "manifest"}),
            json!({"category": "c4"}),
            json!({"category": "manifest"}),
            json!({"category": "instruction-rules"}),
        ];
        let cats = doctor_repair_categories(&repairs);
        assert_eq!(cats, vec!["manifest", "c4", "instruction-rules"]);
    }

    #[test]
    fn apply_doctor_repairs_empty_status() {
        let td = temp_dir();
        let status = json!({ "doctorRepairs": [] });
        let result = apply_doctor_repairs(td.path(), &status, false, false);
        assert!(result.is_empty());
    }

    fn write_release_truth_target(dir: &Path, with_index: Option<&str>) {
        write(
            dir,
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        write(
            dir,
            "docs/architext/data/releases/v1-0-0.json",
            r#"{ "id": "v1-0-0", "version": "1.0.0", "name": "One", "status": "completed",
                 "posture": "shipped", "releasedAt": "2026-01-01T00:00:00.000Z",
                 "lastUpdated": "2026-01-01T00:00:00.000Z", "summary": "First release.",
                 "scope": { "required": [], "planned": [], "stretch": [], "deferred": [], "outOfScope": [] },
                 "workstreams": [], "blockers": [], "milestones": [], "dependencies": [], "evidence": [] }"#,
        );
        if let Some(index) = with_index {
            write(dir, "docs/architext/data/releases/index.json", index);
        }
    }

    #[test]
    fn repair_release_truth_creates_missing_index_from_details() {
        // Status advertises "create missing Release Truth history index" when the
        // configured index file is absent; the apply side must actually create it,
        // not silently return no changes.
        let td = temp_dir();
        write_release_truth_target(td.path(), None);

        let changes = repair_release_truth_data(td.path(), false);
        assert_eq!(changes, vec!["create missing Release Truth history index".to_string()]);

        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index.json created");
        assert_eq!(index["currentReleaseId"], "v1-0-0");
        assert_eq!(index["releases"][0]["file"], "v1-0-0.json");
    }

    #[test]
    fn repair_release_truth_missing_index_dry_run_reports_without_writing() {
        let td = temp_dir();
        write_release_truth_target(td.path(), None);

        let changes = repair_release_truth_data(td.path(), true);
        assert_eq!(changes, vec!["create missing Release Truth history index".to_string()]);
        assert!(!td.path().join("docs/architext/data/releases/index.json").exists());
    }

    #[test]
    fn repair_release_truth_regenerates_unparseable_index() {
        // Status treats an unreadable index the same as a missing one; apply must too.
        let td = temp_dir();
        write_release_truth_target(td.path(), Some("{ not json"));

        let changes = repair_release_truth_data(td.path(), false);
        assert_eq!(changes, vec!["create missing Release Truth history index".to_string()]);

        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index.json regenerated");
        assert_eq!(index["currentReleaseId"], "v1-0-0");
    }
}
