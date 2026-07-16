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
                // Same union enumerator as the exists-branch (empty named set) so
                // id-dedup semantics are uniform across both paths.
                let detail_entries = release_detail_entries(release_dir, &index_path, &Value::Null);
                let generated = release::generated_release_index(&Value::Null, &detail_entries);
                if !dry_run {
                    let _ = std::fs::create_dir_all(release_dir);
                    let _ = write_json(&index_path, &generated);
                }
                return vec!["create missing Release Truth history index".to_string()];
            }
        };
        let detail_entries = release_detail_entries(release_dir, &index_path, &index);
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
/// case where no readable index exists to enumerate them. (New Rust-side behavior
/// closing an apply-side gap — the JS `applyDoctorRepairs` had no equivalent and
/// skipped this repair.) A `*.json` file counts as a detail only if it parses and
/// carries every summary-source field (`id`, `version`, `name`, `status`,
/// `posture`, `summary`, `lastUpdated`) as a non-blank string — the scanned
/// directory can hold unrelated or half-written JSON (manifest.json when the
/// index lives in the data root, tool or editor artifacts, partial stubs), which
/// must not be swept into the regenerated index as null-field entries. The check
/// is deliberately presence-only, not enum/pattern validation: a detail with an
/// invalid status or id is malformed release HISTORY that post-repair validation
/// must flag loudly, not data for this scan to silently drop. The index file is
/// excluded by name; unreadable files are skipped. Sorted by file name for
/// deterministic output.
fn release_detail_entries_from_dir(release_dir: &Path, index_path: &Path) -> Value {
    let index_file_name = index_path.file_name();
    let mut files: Vec<String> = std::fs::read_dir(release_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.file_name() == index_file_name
                || path.extension().and_then(|e| e.to_str()) != Some("json")
            {
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
            let looks_like_detail = ["id", "version", "name", "status", "posture", "summary", "lastUpdated"]
                .iter()
                .all(|field| detail[*field].as_str().is_some_and(|s| !s.trim().is_empty()));
            if !looks_like_detail {
                return None;
            }
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

/// Enumerate release detail entries as the union of files the index names and
/// details discovered on disk. Index-named files are authoritative — enumerated
/// even when they would not pass the dir-scan's strict shape filter, so an
/// imperfect-but-indexed detail surfaces via validation instead of silently
/// vanishing from the index on regeneration. Dir-discovered files not named by
/// the index (a detail added on disk but never indexed — the unreachable
/// "add <id> to Release Truth history" case) go through the strict filter.
/// Used by both the status side (`generated_release_history_changes`) and the
/// apply side so advertised and applied repairs always agree.
///
/// Dedup is by normalized joined path (the index may spell a file
/// "./v1.json" or point into a subdirectory) AND by release id (index-named
/// entries win, then sorted scan order) — validation has no duplicate-id
/// check, so an id entering twice would be written as silent corruption.
pub fn release_detail_entries(release_dir: &Path, index_path: &Path, index: &Value) -> Value {
    // Components-normalized join: drops "." segments so "./v1.json" and
    // "v1.json" compare equal, and subdirectory spellings stay distinct.
    let normalize = |file: &str| -> std::path::PathBuf {
        release_dir.join(file).components().collect()
    };

    let indexed = build_release_detail_entries(release_dir, index);
    let mut entries = indexed.as_array().cloned().unwrap_or_default();
    let named_paths: std::collections::HashSet<std::path::PathBuf> = index["releases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["file"].as_str().map(normalize))
        .collect();
    let mut seen_ids: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|e| e["detail"]["id"].as_str().map(|s| s.to_string()))
        .collect();
    for entry in release_detail_entries_from_dir(release_dir, index_path)
        .as_array()
        .into_iter()
        .flatten()
    {
        let file = entry["file"].as_str().unwrap_or_default();
        if named_paths.contains(&normalize(file)) {
            continue;
        }
        let id = entry["detail"]["id"].as_str().unwrap_or_default();
        if !seen_ids.insert(id.to_string()) {
            continue;
        }
        entries.push(entry.clone());
    }
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
    fn repair_release_truth_dir_scan_skips_non_release_json() {
        // The dir scan must not ingest arbitrary JSON as release details: with the
        // index configured in the data root, manifest.json itself sits in the
        // scanned directory (QA repro), and stray tool/editor JSON can sit in
        // releases/. Anything without a non-empty id AND version is not a detail.
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "release-index.json" } }"#,
        );
        write(
            td.path(),
            "docs/architext/data/v1-0-0.json",
            r#"{ "id": "v1-0-0", "version": "1.0.0", "name": "One", "status": "completed",
                 "posture": "shipped", "lastUpdated": "2026-01-01T00:00:00.000Z", "summary": "First.",
                 "scope": { "required": [], "planned": [], "stretch": [], "deferred": [], "outOfScope": [] },
                 "workstreams": [], "blockers": [], "milestones": [], "dependencies": [], "evidence": [] }"#,
        );
        write(td.path(), "docs/architext/data/schema-notes.json", r#"{ "notes": "not a release" }"#);
        // Partial stubs carrying only some summary-source fields must be excluded
        // too — indexing them writes null-field entries that fail schema validation.
        write(
            td.path(),
            "docs/architext/data/draft-stub.json",
            r#"{ "id": "draft", "version": "0.0.1" }"#,
        );
        write(
            td.path(),
            "docs/architext/data/blank-version.json",
            r#"{ "id": "blank", "version": "   ", "name": "Blank", "status": "planned",
                 "posture": "on-track", "summary": "s", "lastUpdated": "2026-01-01T00:00:00.000Z" }"#,
        );

        let changes = repair_release_truth_data(td.path(), false);
        assert_eq!(changes, vec!["create missing Release Truth history index".to_string()]);

        let index = read_json(&td.path().join("docs/architext/data/release-index.json"))
            .expect("index created");
        let releases = index["releases"].as_array().unwrap();
        assert_eq!(releases.len(), 1, "only the real release detail is indexed: {releases:?}");
        assert_eq!(releases[0]["id"], "v1-0-0");
        assert_eq!(index["currentReleaseId"], "v1-0-0");
    }

    #[test]
    fn repair_release_truth_multi_release_picks_latest_current() {
        let td = temp_dir();
        write_release_truth_target(td.path(), None);
        write(
            td.path(),
            "docs/architext/data/releases/v2-0-0.json",
            r#"{ "id": "v2-0-0", "version": "2.0.0", "name": "Two", "status": "completed",
                 "posture": "shipped", "releasedAt": "2026-02-01T00:00:00.000Z",
                 "lastUpdated": "2026-02-01T00:00:00.000Z", "summary": "Second release.",
                 "scope": { "required": [], "planned": [], "stretch": [], "deferred": [], "outOfScope": [] },
                 "workstreams": [], "blockers": [], "milestones": [], "dependencies": [], "evidence": [] }"#,
        );

        let changes = repair_release_truth_data(td.path(), false);
        assert_eq!(changes, vec!["create missing Release Truth history index".to_string()]);

        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index created");
        assert_eq!(index["releases"].as_array().unwrap().len(), 2);
        assert_eq!(index["currentReleaseId"], "v2-0-0");
    }

    #[test]
    fn repair_release_truth_adds_on_disk_detail_missing_from_index() {
        // Field report (roboticus, 2026-07-16): a detail file on disk but omitted
        // from an existing index was invisible — enumeration read files FROM the
        // index, so the "add <id> to Release Truth history" repair was
        // unreachable. The enumeration must union index-named files with
        // dir-discovered ones.
        let td = temp_dir();
        write_release_truth_target(
            td.path(),
            Some(
                r#"{ "currentReleaseId": "v1-0-0", "releases": [] }"#,
            ),
        );

        let changes = repair_release_truth_data(td.path(), false);
        assert!(
            changes.iter().any(|c| c.contains("add v1-0-0 to Release Truth history")),
            "expected add-change, got: {changes:?}"
        );

        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index rewritten");
        assert_eq!(index["releases"][0]["id"], "v1-0-0");
    }

    #[test]
    fn repair_release_truth_existing_index_scan_still_skips_junk() {
        // The union scan must not weaken the cp-2/cp-3 contamination guarantees
        // when the index exists: stray JSON stays out.
        let td = temp_dir();
        write_release_truth_target(
            td.path(),
            Some(r#"{ "currentReleaseId": "v1-0-0", "releases": [] }"#),
        );
        write(
            td.path(),
            "docs/architext/data/releases/draft-stub.json",
            r#"{ "id": "draft", "version": "0.0.1" }"#,
        );

        repair_release_truth_data(td.path(), false);
        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index rewritten");
        let ids: Vec<&str> = index["releases"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["id"].as_str())
            .collect();
        assert_eq!(ids, vec!["v1-0-0"], "stub must not be indexed: {ids:?}");
    }

    #[test]
    fn repair_release_truth_index_named_files_trusted_over_scan_filter() {
        // Files the index itself names are authoritative: they stay enumerated
        // even when they would not pass the dir-scan's strict shape filter
        // (imperfect details must surface via validation, not vanish from the
        // index on regeneration).
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        // Partial detail: would fail the strict filter (no posture/lastUpdated),
        // but the index names it. Give the index a stale summary so regeneration
        // has a change to make.
        write(
            td.path(),
            "docs/architext/data/releases/v0-1-0.json",
            r#"{ "id": "v0-1-0", "version": "0.1.0", "name": "Partial", "status": "planned",
                 "summary": "stub", "scope": { "required": [] } }"#,
        );
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v0-1-0", "releases": [
                 { "id": "v0-1-0", "file": "v0-1-0.json", "version": "0.1.0", "name": "STALE",
                   "status": "planned", "summary": "stale" } ] }"#,
        );

        repair_release_truth_data(td.path(), false);
        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index rewritten");
        let ids: Vec<&str> = index["releases"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["id"].as_str())
            .collect();
        assert_eq!(ids, vec!["v0-1-0"], "index-named file must stay enumerated: {ids:?}");
    }

    #[test]
    fn repair_release_truth_dot_prefixed_index_file_not_double_enumerated() {
        // QA cp-4: the index may spell a file "./v1-0-0.json" (schema allows any
        // string); the dir scan names it "v1-0-0.json". Dedup must compare
        // normalized paths, not raw strings, or the same file is enumerated
        // twice and the regenerated index carries duplicate ids.
        let td = temp_dir();
        write_release_truth_target(td.path(), None);
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v1-0-0", "releases": [
                 { "id": "v1-0-0", "file": "./v1-0-0.json", "version": "1.0.0", "name": "STALE",
                   "status": "completed", "summary": "stale" } ] }"#,
        );

        repair_release_truth_data(td.path(), false);
        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index rewritten");
        let ids: Vec<&str> = index["releases"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["id"].as_str())
            .collect();
        assert_eq!(ids, vec!["v1-0-0"], "same file must not enter twice: {ids:?}");
    }

    #[test]
    fn repair_release_truth_same_id_different_file_not_duplicated() {
        // QA cp-4: a dir-discovered detail sharing an id with an index-named one
        // (e.g. a leftover copy after a rename) must not produce duplicate-id
        // index entries — validation has no dup-id check, so doctor would be
        // writing silent corruption. Index-named wins.
        let td = temp_dir();
        write_release_truth_target(td.path(), None);
        write(
            td.path(),
            "docs/architext/data/releases/copy-of-v1-0-0.json",
            r#"{ "id": "v1-0-0", "version": "1.0.0", "name": "Copy", "status": "completed",
                 "posture": "shipped", "releasedAt": "2026-01-01T00:00:00.000Z",
                 "lastUpdated": "2026-01-01T00:00:00.000Z", "summary": "leftover copy.",
                 "scope": { "required": [], "planned": [], "stretch": [], "deferred": [], "outOfScope": [] },
                 "workstreams": [], "blockers": [], "milestones": [], "dependencies": [], "evidence": [] }"#,
        );
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v1-0-0", "releases": [
                 { "id": "v1-0-0", "file": "v1-0-0.json", "version": "1.0.0", "name": "STALE",
                   "status": "completed", "summary": "stale" } ] }"#,
        );

        repair_release_truth_data(td.path(), false);
        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index rewritten");
        let entries: Vec<(&str, &str)> = index["releases"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| Some((r["id"].as_str()?, r["file"].as_str()?)))
            .collect();
        assert_eq!(
            entries,
            vec![("v1-0-0", "v1-0-0.json")],
            "index-named file wins; no duplicate ids: {entries:?}"
        );
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
