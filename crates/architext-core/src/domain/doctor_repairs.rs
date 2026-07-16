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
                // Same plan machinery with a null index (empty named set) so
                // scan and id-dedup semantics are uniform across both paths. An
                // unparseable index is backed up before being replaced (E-1).
                let plan = release_truth_repair_plan(release_dir, &index_path, &Value::Null);
                if !dry_run {
                    let _ = std::fs::create_dir_all(release_dir);
                    if index_path.exists() {
                        let _ = backup_file(&index_path);
                    }
                    let _ = write_json(&index_path, &plan.generated);
                }
                return vec!["create missing Release Truth history index".to_string()];
            }
        };
        let plan = release_truth_repair_plan(release_dir, &index_path, &index);
        if plan.changes.is_empty() {
            return vec![];
        }
        if !dry_run {
            // E-1 (human directive): every file this repair overwrites is first
            // backed up with a timestamped name; recovered details are written
            // re-marshalled so the regenerated index is derived from valid data;
            // and each recovery is recorded in repair-advice.json because
            // reconciling old (backup) vs new (recovered) content is the
            // maintaining agent's responsibility, not the repair's.
            let mut advice_entries: Vec<Value> = Vec::new();
            for (file, detail, backfilled) in &plan.normalized {
                let path = release_dir.join(file);
                let backup_name = if path.exists() { backup_file(&path) } else { None };
                let _ = write_json(&path, detail);
                let rel_dir = release_dir
                    .strip_prefix(&target_data_dir)
                    .unwrap_or_else(|_| Path::new(""));
                advice_entries.push(json!({
                    "kind": "release-detail-recovery",
                    "file": rel_dir.join(file).to_string_lossy(),
                    "backup": backup_name
                        .map(|b| rel_dir.join(b).to_string_lossy().to_string()),
                    "backfilledFields": backfilled,
                    "generatedAt": chrono_now(),
                    "instruction": RECONCILE_INSTRUCTION,
                }));
            }
            if plan.index_changed {
                let _ = backup_file(&index_path);
                let _ = write_json(&index_path, &plan.generated);
            }
            if !advice_entries.is_empty() {
                append_repair_advice(&target_data_dir, advice_entries);
            }
        }
        return plan.changes;
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

fn now_secs() -> u64 {
    // Use std time only; no chrono dep needed for a timestamp string
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn chrono_now() -> String {
    // Format as ISO 8601 — approximate (no sub-second, no tz offset awareness)
    let (y, mo, d, h, mi, sec) = secs_to_datetime(now_secs());
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

// ─── release detail enumeration + repair plan ────────────────────────────────

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
            if !is_complete_detail(&detail) {
                return None;
            }
            Some(json!({ "file": file, "detail": detail }))
        })
        .collect();
    Value::Array(entries)
}

/// The summary-source fields every release detail must carry as non-blank
/// strings for the generated index entry to be schema-valid.
const DETAIL_SUMMARY_FIELDS: [&str; 7] =
    ["id", "version", "name", "status", "posture", "summary", "lastUpdated"];

fn is_complete_detail(detail: &Value) -> bool {
    DETAIL_SUMMARY_FIELDS
        .iter()
        .all(|field| detail[*field].as_str().is_some_and(|s| !s.trim().is_empty()))
}

/// Best-effort recovery of an index-named release detail (E-1 human directive):
/// keep everything the original carries, backfill missing/blank summary-source
/// fields from the index summary, then from explicit defaults, and ensure the
/// schema-required containers exist — so the re-marshalled detail (and the
/// index generated from it) is always schema-valid. Returns the recovered
/// detail plus the list of fields that were backfilled, which the repair
/// records as reconciliation advice for the maintaining agent.
fn normalize_release_detail(
    detail: Option<&Value>,
    summary: &Value,
    now: &str,
) -> (Value, Vec<String>) {
    let mut backfilled: Vec<String> = Vec::new();
    let mut obj = detail
        .and_then(|d| d.as_object().cloned())
        .unwrap_or_default();

    let recovered_id = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| summary["id"].as_str().filter(|s| !s.trim().is_empty()))
        .unwrap_or("recovered-release")
        .to_string();

    let defaults: [(&str, String); 7] = [
        ("id", recovered_id.clone()),
        ("version", "0.0.0".to_string()),
        ("name", recovered_id.clone()),
        ("status", "draft".to_string()),
        ("posture", "at-risk".to_string()),
        (
            "summary",
            "Recovered by architext doctor from an incomplete release detail; review and complete."
                .to_string(),
        ),
        ("lastUpdated", now.to_string()),
    ];
    for (field, default) in defaults {
        let blank = obj
            .get(field)
            .and_then(Value::as_str)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if blank {
            let value = summary[field]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .unwrap_or(default);
            obj.insert(field.to_string(), Value::String(value));
            backfilled.push(field.to_string());
        }
    }
    for optional in ["releasedAt", "targetDate", "targetWindow"] {
        if !obj.contains_key(optional) {
            if let Some(s) = summary[optional].as_str() {
                obj.insert(optional.to_string(), Value::String(s.to_string()));
                backfilled.push(optional.to_string());
            }
        }
    }
    // Validation requires a target for draft/planned/implementing releases. A
    // placeholder window is an honest plan statement demanding review; a
    // fabricated releasedAt for a completed release would be invented history,
    // so that case is deliberately left for validation to flag.
    let status = obj.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(status, "draft" | "planned" | "implementing")
        && !obj.contains_key("targetDate")
        && !obj.contains_key("targetWindow")
    {
        obj.insert(
            "targetWindow".to_string(),
            Value::String("TBD — set a target window (recovered detail)".to_string()),
        );
        backfilled.push("targetWindow".to_string());
    }
    if !obj.get("scope").map(Value::is_object).unwrap_or(false) {
        obj.insert("scope".to_string(), json!({}));
    }
    if let Some(scope) = obj.get_mut("scope").and_then(Value::as_object_mut) {
        for section in ["required", "planned", "stretch", "deferred", "outOfScope"] {
            if !scope.get(section).map(Value::is_array).unwrap_or(false) {
                scope.insert(section.to_string(), json!([]));
            }
        }
    }
    for container in ["workstreams", "blockers", "milestones", "dependencies", "evidence"] {
        if !obj.get(container).map(Value::is_array).unwrap_or(false) {
            obj.insert(container.to_string(), json!([]));
            backfilled.push(container.to_string());
        }
    }
    (Value::Object(obj), backfilled)
}

/// Standing instruction recorded with every recovery: the repair produces a
/// mechanically valid file, but only the maintaining agent can restore the
/// real facts.
const RECONCILE_INSTRUCTION: &str = "architext doctor recovered this file with \
backfilled placeholder values. Reconciling recovered content is YOUR \
responsibility: restore real facts from the timestamped backup, the source \
code, and git history; replace every placeholder; run `architext validate`; \
then delete the backup file and remove this advice entry.";

/// Append recovery entries to `docs/architext/data/repair-advice.json`,
/// creating it if absent and preserving unresolved entries from earlier runs.
/// The maintaining agent removes entries as it reconciles them.
fn append_repair_advice(target_data_dir: &Path, new_entries: Vec<Value>) {
    let advice_path = target_data_dir.join("repair-advice.json");
    let mut advice = read_json(&advice_path)
        .filter(|v| v["advice"].is_array())
        .unwrap_or_else(|| json!({ "advice": [] }));
    if let Some(list) = advice["advice"].as_array_mut() {
        list.extend(new_entries);
    }
    let _ = write_json(&advice_path, &advice);
}

/// Copy `path` to a timestamped sibling (`<name>.<yyyymmddThhmmssZ>.bak`)
/// before it is overwritten. The `.bak` extension keeps backups out of the
/// `*.json` dir scan. Returns the backup file name on success.
fn backup_file(path: &Path) -> Option<String> {
    let (y, mo, d, h, mi, s) = secs_to_datetime(now_secs());
    let name = format!(
        "{}.{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z.bak",
        path.file_name()?.to_str()?
    );
    let dest = path.with_file_name(&name);
    std::fs::copy(path, &dest).ok()?;
    Some(name)
}

/// The full release-truth repair plan: what to say (changes), which details to
/// recover (normalized), and the index to write (generated). Computed
/// identically by the status side and the apply side so advertised and applied
/// repairs cannot diverge.
pub struct ReleaseTruthPlan {
    pub changes: Vec<String>,
    /// (release-dir-relative file, recovered detail, backfilled field names)
    pub normalized: Vec<(String, Value, Vec<String>)>,
    pub entries: Value,
    pub generated: Value,
    pub index_changed: bool,
}

pub fn release_truth_repair_plan(
    release_dir: &Path,
    index_path: &Path,
    index: &Value,
) -> ReleaseTruthPlan {
    let now = chrono_now();
    // Components-normalized join: drops "." segments so "./v1.json" and
    // "v1.json" compare equal, and subdirectory spellings stay distinct.
    let normalize_path = |file: &str| -> std::path::PathBuf {
        release_dir.join(file).components().collect()
    };

    let mut entries: Vec<Value> = Vec::new();
    let mut normalized: Vec<(String, Value, Vec<String>)> = Vec::new();
    let mut changes: Vec<String> = Vec::new();
    let mut named_paths: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Index-named files first: kept even when imperfect, but imperfect ones are
    // recovered (backfilled + re-marshalled) rather than summarized with nulls.
    // Dangling references (file deleted) drop out: deleting a detail file is an
    // intentional removal.
    for summary in index["releases"].as_array().into_iter().flatten() {
        let Some(file) = summary["file"].as_str() else { continue };
        named_paths.insert(normalize_path(file));
        let path = release_dir.join(file);
        if !path.exists() {
            continue;
        }
        let raw = read_json(&path);
        let detail = match raw {
            Some(d) if is_complete_detail(&d) => d,
            other => {
                let (norm, backfilled) = normalize_release_detail(other.as_ref(), summary, &now);
                changes.push(format!("normalize incomplete release detail {file}"));
                normalized.push((file.to_string(), norm.clone(), backfilled));
                norm
            }
        };
        if let Some(id) = detail["id"].as_str() {
            seen_ids.insert(id.to_string());
        }
        entries.push(json!({ "file": file, "detail": detail }));
    }

    // Dir-discovered details the index omits (strict shape filter), deduped by
    // normalized path and by id — validation has no duplicate-id check, so an
    // id entering twice would be written as silent corruption.
    for entry in release_detail_entries_from_dir(release_dir, index_path)
        .as_array()
        .into_iter()
        .flatten()
    {
        let file = entry["file"].as_str().unwrap_or_default();
        if named_paths.contains(&normalize_path(file)) {
            continue;
        }
        let id = entry["detail"]["id"].as_str().unwrap_or_default();
        if !seen_ids.insert(id.to_string()) {
            continue;
        }
        entries.push(entry.clone());
    }

    let entries = Value::Array(entries);
    let generated = release::generated_release_index(index, &entries);
    let gen_changes: Vec<String> = release::release_index_generation_changes(index, &generated)
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let index_changed = !gen_changes.is_empty();
    changes.extend(gen_changes);

    ReleaseTruthPlan { changes, normalized, entries, generated, index_changed }
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

    fn bak_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| n.ends_with(".bak"))
            .collect()
    }

    #[test]
    fn repair_release_truth_normalizes_incomplete_indexed_detail() {
        // E-1 (human directive): an index-named detail that is incomplete is
        // recovered — original backed up timestamped, missing fields backfilled
        // from the index summary then defaults, normalized detail rewritten —
        // so the regenerated index never carries null required fields.
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
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
                 { "id": "v0-1-0", "file": "v0-1-0.json", "version": "0.1.0", "name": "Partial",
                   "status": "planned", "posture": "on-track", "summary": "stub" } ] }"#,
        );

        let changes = repair_release_truth_data(td.path(), false);
        assert!(
            changes.iter().any(|c| c.contains("normalize incomplete release detail v0-1-0.json")),
            "expected a normalize change, got: {changes:?}"
        );

        let releases_dir = td.path().join("docs/architext/data/releases");
        let backups = bak_files(&releases_dir);
        assert!(
            backups.iter().any(|b| b.starts_with("v0-1-0.json.")),
            "original detail must be backed up: {backups:?}"
        );

        let detail = read_json(&releases_dir.join("v0-1-0.json")).expect("normalized detail");
        // Backfilled from the index summary where available…
        assert_eq!(detail["posture"], "on-track");
        // …and from defaults where nothing else is known.
        assert!(detail["lastUpdated"].as_str().is_some_and(|s| !s.is_empty()));
        for container in ["workstreams", "blockers", "milestones", "dependencies", "evidence"] {
            assert!(detail[container].is_array(), "{container} must exist");
        }

        let index = read_json(&releases_dir.join("index.json")).expect("index rewritten");
        let entry = &index["releases"][0];
        assert_eq!(entry["id"], "v0-1-0");
        assert!(entry["posture"].as_str().is_some(), "no null posture in index: {entry}");
        assert!(entry["lastUpdated"].as_str().is_some(), "no null lastUpdated in index");
    }

    #[test]
    fn repair_release_truth_recovers_unparseable_indexed_detail() {
        // An index-named detail that no longer parses is recovered from the
        // index summary (plus defaults); the corrupt original is backed up.
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        write(td.path(), "docs/architext/data/releases/v0-2-0.json", "{ not json");
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v0-2-0", "releases": [
                 { "id": "v0-2-0", "file": "v0-2-0.json", "version": "0.2.0", "name": "Corrupt",
                   "status": "completed", "posture": "shipped", "summary": "was fine once",
                   "lastUpdated": "2026-01-01T00:00:00.000Z" } ] }"#,
        );

        let changes = repair_release_truth_data(td.path(), false);
        assert!(
            changes.iter().any(|c| c.contains("normalize incomplete release detail v0-2-0.json")),
            "expected recovery change, got: {changes:?}"
        );

        let releases_dir = td.path().join("docs/architext/data/releases");
        assert!(
            bak_files(&releases_dir).iter().any(|b| b.starts_with("v0-2-0.json.")),
            "corrupt original must be backed up"
        );
        let detail = read_json(&releases_dir.join("v0-2-0.json")).expect("recovered detail parses");
        assert_eq!(detail["id"], "v0-2-0");
        assert_eq!(detail["posture"], "shipped");

        let index = read_json(&releases_dir.join("index.json")).expect("index");
        assert_eq!(index["releases"][0]["id"], "v0-2-0", "entry retained, not dropped");
    }

    #[test]
    fn repair_release_truth_backs_up_index_before_rewrite() {
        let td = temp_dir();
        write_release_truth_target(
            td.path(),
            Some(r#"{ "currentReleaseId": "v1-0-0", "releases": [] }"#),
        );

        repair_release_truth_data(td.path(), false);
        let releases_dir = td.path().join("docs/architext/data/releases");
        assert!(
            bak_files(&releases_dir).iter().any(|b| b.starts_with("index.json.")),
            "index must be backed up before rewrite"
        );
        // Backups themselves must never be scanned as details on later runs.
        let changes_again = repair_release_truth_data(td.path(), false);
        assert!(changes_again.is_empty(), "second run stable, got: {changes_again:?}");
    }

    #[test]
    fn repair_release_truth_dry_run_normalization_writes_nothing() {
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        write(
            td.path(),
            "docs/architext/data/releases/v0-1-0.json",
            r#"{ "id": "v0-1-0", "version": "0.1.0" }"#,
        );
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v0-1-0", "releases": [
                 { "id": "v0-1-0", "file": "v0-1-0.json", "version": "0.1.0" } ] }"#,
        );
        let before = std::fs::read_to_string(td.path().join("docs/architext/data/releases/v0-1-0.json")).unwrap();

        let changes = repair_release_truth_data(td.path(), true);
        assert!(changes.iter().any(|c| c.contains("normalize incomplete release detail")));

        let releases_dir = td.path().join("docs/architext/data/releases");
        assert!(bak_files(&releases_dir).is_empty(), "dry run must not create backups");
        let after = std::fs::read_to_string(releases_dir.join("v0-1-0.json")).unwrap();
        assert_eq!(before, after, "dry run must not rewrite the detail");
    }

    #[test]
    fn repair_release_truth_writes_reconciliation_advice() {
        // E-1 addendum (human directive): recovery is not the end state — the
        // repair must record, machine-readably, that reconciling old (backup)
        // vs new (normalized) content is the maintaining agent's
        // responsibility, naming the backup and the backfilled fields.
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
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
                 { "id": "v0-1-0", "file": "v0-1-0.json", "version": "0.1.0", "name": "Partial",
                   "status": "planned", "posture": "on-track", "summary": "stub" } ] }"#,
        );

        repair_release_truth_data(td.path(), false);

        let advice_path = td.path().join("docs/architext/data/repair-advice.json");
        let advice = read_json(&advice_path).expect("repair-advice.json written");
        let entries = advice["advice"].as_array().expect("advice array");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry["file"], "releases/v0-1-0.json");
        let backup = entry["backup"].as_str().expect("backup recorded");
        assert!(backup.starts_with("releases/v0-1-0.json."), "backup path: {backup}");
        assert!(td.path().join("docs/architext/data").join(backup).exists(), "backup on disk");
        let backfilled: Vec<&str> = entry["backfilledFields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(backfilled.contains(&"posture") && backfilled.contains(&"lastUpdated"),
            "backfilled fields recorded: {backfilled:?}");
        assert!(entry["instruction"].as_str().is_some_and(|s| s.contains("responsibility")));
    }

    #[test]
    fn repair_release_truth_appends_advice_and_stays_stable() {
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        write(td.path(), "docs/architext/data/releases/v0-2-0.json", "{ not json");
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v0-2-0", "releases": [
                 { "id": "v0-2-0", "file": "v0-2-0.json", "version": "0.2.0", "name": "Corrupt",
                   "status": "completed", "posture": "shipped", "summary": "was fine",
                   "lastUpdated": "2026-01-01T00:00:00.000Z" } ] }"#,
        );
        // Pre-existing advice from an earlier repair must survive.
        write(
            td.path(),
            "docs/architext/data/repair-advice.json",
            r#"{ "advice": [ { "kind": "release-detail-recovery", "file": "releases/old.json",
                 "backup": "releases/old.json.20260101T000000Z.bak",
                 "backfilledFields": ["summary"], "instruction": "reconcile" } ] }"#,
        );

        repair_release_truth_data(td.path(), false);
        let advice = read_json(&td.path().join("docs/architext/data/repair-advice.json")).unwrap();
        assert_eq!(advice["advice"].as_array().unwrap().len(), 2, "appended, not replaced");

        // A stable second run must not grow the advice ledger.
        let changes = repair_release_truth_data(td.path(), false);
        assert!(changes.is_empty(), "second run stable: {changes:?}");
        let advice = read_json(&td.path().join("docs/architext/data/repair-advice.json")).unwrap();
        assert_eq!(advice["advice"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn repair_release_truth_dry_run_writes_no_advice() {
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        write(
            td.path(),
            "docs/architext/data/releases/v0-1-0.json",
            r#"{ "id": "v0-1-0", "version": "0.1.0" }"#,
        );
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "v0-1-0", "releases": [
                 { "id": "v0-1-0", "file": "v0-1-0.json", "version": "0.1.0" } ] }"#,
        );

        repair_release_truth_data(td.path(), true);
        assert!(!td.path().join("docs/architext/data/repair-advice.json").exists());
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
