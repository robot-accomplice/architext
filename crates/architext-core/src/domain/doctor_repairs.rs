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
//! I/O uses `json_write::{read_json, write_json}`; release-truth backup,
//! advice-ledger, and enumeration I/O lives in `domain::release_recovery`.
//! The output is a Vec of `DoctorRepair` records, one per change, each
//! carrying `status: applied|failed` (additive over the JS return shape).

use std::path::Path;

use serde_json::{json, Value};

use crate::domain::release_recovery::{
    append_repair_advice, backup_file, chrono_now, release_truth_repair_plan,
    RECONCILE_INSTRUCTION,
};
use crate::domain::{c4_quality, instruction_rules, schema_migration};
use crate::json_write::{read_json, write_json};

pub const DATA_SCHEMA_VERSION: &str = "1.5.0";

/// A single repair action applied (extends the JS `{ category, file, summary }`
/// shape with an additive `status`/`error`: a failed write must never be
/// reported as an applied change — swarm ticket T-1, Rule 13 fail-loud).
#[derive(Debug, Clone)]
pub struct DoctorRepair {
    pub category: String,
    pub file: String,
    pub summary: String,
    pub error: Option<String>,
}

impl DoctorRepair {
    pub fn to_json(&self) -> Value {
        let mut obj = json!({
            "category": self.category,
            "file": self.file,
            "summary": self.summary,
            "status": if self.error.is_none() { "applied" } else { "failed" }
        });
        if let Some(err) = &self.error {
            obj["error"] = Value::String(err.clone());
        }
        obj
    }
}

/// One change from a repair category fn: the human summary, the write error if
/// applying it failed (always `None` under `--dry-run`), and — when the change
/// touched a different file than the category's default — the actual path
/// written, so the reported `file` never misleads (code-rca B-2: instruction
/// rewrites were attributed to rules.json, hiding the true write target).
#[derive(Debug, Clone)]
pub struct RepairOutcome {
    pub summary: String,
    pub error: Option<String>,
    pub file: Option<String>,
}

fn applied(summaries: Vec<String>, error: Option<String>) -> Vec<RepairOutcome> {
    summaries
        .into_iter()
        .map(|summary| RepairOutcome { summary, error: error.clone(), file: None })
        .collect()
}

// ─── Path helpers (mirrors JS `dataDir(target)`) ─────────────────────────────

fn data_dir(target: &Path) -> std::path::PathBuf {
    target.join("docs").join("architext").join("data")
}


// ─── repairC4Data ─────────────────────────────────────────────────────────────

/// Port of `repairC4Data(target, dryRun)`.
pub fn repair_c4_data(target: &Path, dry_run: bool) -> Vec<RepairOutcome> {
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

    let mut error = None;
    if !changes.is_empty() && !dry_run {
        let mut new_doc = views_doc.clone();
        new_doc.as_object_mut().unwrap().insert(
            "views".to_string(),
            repaired["views"].clone(),
        );
        error = write_json(&views_path, &new_doc).err().map(|e| e.to_string());
    }

    applied(changes, error)
}

// ─── repairManifestData ───────────────────────────────────────────────────────

/// Port of `repairManifestData(target, dryRun)`.
pub fn repair_manifest_data(target: &Path, dry_run: bool) -> Vec<RepairOutcome> {
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

    let mut error = None;
    if !repair_changes.is_empty() && !dry_run {
        let mut new_manifest = manifest.clone();
        new_manifest
            .as_object_mut()
            .unwrap()
            .insert("schemaVersion".to_string(), Value::String(DATA_SCHEMA_VERSION.to_string()));
        error = write_json(&manifest_path, &new_manifest).err().map(|e| e.to_string());
    }

    applied(repair_changes, error)
}

// ─── repairReleaseTruthData ───────────────────────────────────────────────────

/// Port of `repairReleaseTruthData(target, dryRun)`.
pub fn repair_release_truth_data(target: &Path, dry_run: bool) -> Vec<RepairOutcome> {
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
                let mut error = None;
                if !dry_run {
                    let _ = std::fs::create_dir_all(release_dir);
                    if index_path.exists() {
                        if let Err(e) = backup_file(&index_path) {
                            // Never overwrite the (unparseable) original without
                            // a backup — it may be hand-recoverable.
                            return vec![RepairOutcome {
                                summary: "backup of the unparseable release index failed; no release-truth repairs applied"
                                    .to_string(),
                                error: Some(e),
                                file: None,
                            }];
                        }
                    }
                    error = write_json(&index_path, &plan.generated).err().map(|e| e.to_string());
                }
                return vec![RepairOutcome {
                    summary: "create missing Release Truth history index".to_string(),
                    error,
                    file: None,
                }];
            }
        };
        let plan = release_truth_repair_plan(release_dir, &index_path, &index);
        if plan.changes.is_empty() {
            return vec![];
        }
        if dry_run {
            return applied(plan.changes, None);
        }
        {
            // E-1 (human directive): every file this repair overwrites is first
            // backed up with a timestamped name; recovered details are written
            // re-marshalled so the regenerated index is derived from valid data;
            // and each recovery is recorded in repair-advice.json because
            // reconciling old (backup) vs new (recovered) content is the
            // maintaining agent's responsibility, not the repair's.
            //
            // Backups are taken FIRST, all-or-nothing: if any backup fails, no
            // file is overwritten and the failure is the (loud) result —
            // proceeding would destroy the only copy of an original, inverting
            // the guarantee this feature exists to provide.
            let mut backups: Vec<Option<String>> = Vec::new();
            for (file, _, _) in &plan.normalized {
                let path = release_dir.join(file);
                if !path.exists() {
                    backups.push(None);
                    continue;
                }
                match backup_file(&path) {
                    Ok(name) => backups.push(Some(name)),
                    Err(e) => {
                        return vec![RepairOutcome {
                            summary: format!(
                                "backup of {file} failed; no release-truth repairs applied"
                            ),
                            error: Some(e),
                            file: None,
                        }];
                    }
                }
            }
            if plan.index_changed {
                if let Err(e) = backup_file(&index_path) {
                    let index_name = index_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "release index".to_string());
                    return vec![RepairOutcome {
                        summary: format!(
                            "backup of {index_name} failed; no release-truth repairs applied"
                        ),
                        error: Some(e),
                        file: None,
                    }];
                }
            }

            // Paths in the advice ledger are data-dir-relative; if the release
            // dir unexpectedly falls outside the data dir, record the absolute
            // path rather than a silently-wrong relative one.
            let advice_path_for = |name: &str| -> String {
                match release_dir.strip_prefix(&target_data_dir) {
                    Ok(rel) => rel.join(name).to_string_lossy().to_string(),
                    Err(_) => release_dir.join(name).to_string_lossy().to_string(),
                }
            };
            let mut advice_entries: Vec<Value> = Vec::new();
            let mut detail_errors: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for ((file, detail, backfilled), backup_name) in
                plan.normalized.iter().zip(backups)
            {
                if let Err(e) = write_json(&release_dir.join(file), detail) {
                    detail_errors.insert(file.clone(), e.to_string());
                    continue;
                }
                advice_entries.push(json!({
                    "kind": "release-detail-recovery",
                    "file": advice_path_for(file),
                    "backup": backup_name.map(|b| advice_path_for(&b)),
                    "backfilledFields": backfilled,
                    "generatedAt": chrono_now(),
                    "instruction": RECONCILE_INSTRUCTION,
                }));
            }
            let mut index_error = None;
            if plan.index_changed {
                index_error = write_json(&index_path, &plan.generated).err().map(|e| e.to_string());
            }
            let mut advice_error = None;
            if !advice_entries.is_empty() {
                advice_error = append_repair_advice(&target_data_dir, advice_entries).err();
            }

            // Attach each failure to the change it defeats: a normalize change
            // carries exactly its own detail-write error (matched on the full
            // file key, never a substring — a suffix match mis-stamped
            // successful siblings), everything else (the index regeneration
            // changes) carries the index-write error.
            let mut outcomes: Vec<RepairOutcome> = plan
                .changes
                .into_iter()
                .map(|summary| {
                    let error = match summary.strip_prefix("normalize incomplete release detail ")
                    {
                        Some(file) => detail_errors.get(file).cloned(),
                        None => index_error.clone(),
                    };
                    RepairOutcome { summary, error, file: None }
                })
                .collect();
            if let Some(err) = advice_error {
                outcomes.push(RepairOutcome {
                    summary: "record reconciliation advice in repair-advice.json".to_string(),
                    error: Some(err),
                    file: None,
                });
            }
            return outcomes;
        }
    }

    // manifest.files.releases absent — write starter data
    let repair_changes = vec!["add starter Release Truth data and manifest.files.releases".to_string()];
    let mut error = None;
    if !dry_run {
        let releases_dir = target_data_dir.join("releases");
        let _ = std::fs::create_dir_all(&releases_dir);

        let mut new_manifest = manifest.clone();
        new_manifest["files"]
            .as_object_mut()
            .unwrap()
            .insert("releases".to_string(), Value::String("releases/index.json".to_string()));
        error = write_json(&manifest_path, &new_manifest).err().map(|e| e.to_string());

        if error.is_none() {
            error = write_starter_release_data(&releases_dir);
        }
    }
    applied(repair_changes, error)
}

fn write_starter_release_data(releases_dir: &Path) -> Option<String> {
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
    if let Err(e) = write_json(&releases_dir.join("index.json"), &index) {
        return Some(e.to_string());
    }

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
    write_json(&releases_dir.join(format!("{release_id}.json")), &detail)
        .err()
        .map(|e| e.to_string())
}

// ─── repairInstructionRules ───────────────────────────────────────────────────

/// Port of `repairInstructionRules(target, dryRun)`.
pub fn repair_instruction_rules(target: &Path, dry_run: bool) -> Vec<RepairOutcome> {
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

    if repair_changes.is_empty() {
        return vec![];
    }

    // Per-change outcomes with real file attribution: candidate-rule changes
    // write rules.json (the category default), each rewrite writes its own
    // instruction file. Summaries must stay byte-equal to the advertised
    // repairChanges (both derive from the same migration plan).
    let mut outcomes: Vec<RepairOutcome> = Vec::new();

    let mut rules_error = None;
    let candidates = migration["candidateRules"].as_array().cloned().unwrap_or_default();
    if !candidates.is_empty() && !dry_run {
        let mut new_doc = rules_doc.clone();
        let existing = new_doc["rules"].as_array_mut().unwrap();
        for c in &candidates {
            existing.push(c.clone());
        }
        rules_error = write_json(&rules_path, &new_doc).err().map(|e| e.to_string());
    }
    for c in &candidates {
        outcomes.push(RepairOutcome {
            summary: format!(
                "migrate instruction rule: {}",
                c["title"].as_str().unwrap_or("")
            ),
            error: rules_error.clone(),
            file: None,
        });
    }

    for rewrite in migration["rewriteFiles"].as_array().into_iter().flatten() {
        let Some(path_str) = rewrite["path"].as_str() else { continue };
        let full_path = target.join(path_str);
        let mut error = None;
        if !dry_run {
            if let Some(replacement) = rewrite["replacement"].as_str() {
                // Shared with the convergence gate — see ensure_trailing_newline.
                let content = instruction_rules::ensure_trailing_newline(replacement);
                error = std::fs::write(&full_path, content.as_bytes())
                    .err()
                    .map(|e| e.to_string());
            }
        }
        outcomes.push(RepairOutcome {
            summary: format!(
                "rewrite {path_str} to point at docs/architext/data/rules.json"
            ),
            error,
            file: Some(full_path.to_string_lossy().to_string()),
        });
    }

    outcomes
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
        let changes: Vec<RepairOutcome> = match category.as_str() {
            "c4" => repair_c4_data(target, dry_run),
            "manifest" => repair_manifest_data(target, dry_run),
            "release-truth" => repair_release_truth_data(target, dry_run),
            "instruction-rules" => repair_instruction_rules(target, dry_run),
            _ => vec![],
        };
        let default_file = repair_files(category);
        for outcome in changes {
            applied.push(DoctorRepair {
                category: category.clone(),
                file: outcome.file.unwrap_or_else(|| default_file.clone()),
                summary: outcome.summary,
                error: outcome.error,
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

    fn summaries(outcomes: &[RepairOutcome]) -> Vec<String> {
        outcomes.iter().map(|o| o.summary.clone()).collect()
    }

    #[cfg(unix)]
    #[test]
    fn write_errors_attach_to_the_right_file() {
        // AUDIT/QA cp-9: the summary→error mapping used a suffix match, so a
        // failing "release.json" stamped its error onto the successful
        // "pre-release.json" too (whose summary ends with "release.json"),
        // reporting a successful normalize as failed.
        use std::os::unix::fs::PermissionsExt;
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.5.0", "files": { "releases": "releases/index.json" } }"#,
        );
        write(td.path(), "docs/architext/data/releases/release.json", r#"{ "id": "rel", "version": "1.0.0" }"#);
        write(td.path(), "docs/architext/data/releases/pre-release.json", r#"{ "id": "pre", "version": "1.0.0" }"#);
        write(
            td.path(),
            "docs/architext/data/releases/index.json",
            r#"{ "currentReleaseId": "rel", "releases": [
                 { "id": "rel", "file": "release.json", "version": "1.0.0" },
                 { "id": "pre", "file": "pre-release.json", "version": "1.0.0" } ] }"#,
        );
        let releases_dir = td.path().join("docs/architext/data/releases");
        // Only release.json is unwritable; pre-release.json must stay clean.
        fs::set_permissions(
            releases_dir.join("release.json"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();

        let outcomes = repair_release_truth_data(td.path(), false);

        fs::set_permissions(
            releases_dir.join("release.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let failing = outcomes
            .iter()
            .find(|o| o.summary == "normalize incomplete release detail release.json")
            .expect("outcome for release.json");
        assert!(failing.error.is_some(), "failing file carries its error");
        let succeeding = outcomes
            .iter()
            .find(|o| o.summary == "normalize incomplete release detail pre-release.json")
            .expect("outcome for pre-release.json");
        assert!(
            succeeding.error.is_none(),
            "successful file must not inherit a sibling's error: {succeeding:?}"
        );
    }

    #[test]
    fn instruction_rules_repair_converges_on_second_run() {
        // AUDIT cp-11 F-2: the gate's byte comparison and the writer's
        // normalization share one helper; this integration test pins the
        // load-bearing invariant end to end — apply once against a real temp
        // dir, then a second run must return no outcomes with the tree
        // byte-identical (the B-2 phantom-repair loop).
        let td = temp_dir();
        write(td.path(), "docs/architext/data/rules.json", r#"{ "rules": [] }"#);
        write(
            td.path(),
            "AGENTS.md",
            "# Project\n\nProse.\n\n- Always check tests before committing any code changes\n",
        );

        let first = repair_instruction_rules(td.path(), false);
        assert!(!first.is_empty(), "first run migrates");
        assert!(first.iter().all(|o| o.error.is_none()), "{first:?}");
        // Rewrites attribute to the real file, not the category default.
        assert!(
            first
                .iter()
                .any(|o| o.file.as_deref().is_some_and(|f| f.ends_with("AGENTS.md"))),
            "rewrite outcome carries the written path: {first:?}"
        );
        let agents_after_first = fs::read_to_string(td.path().join("AGENTS.md")).unwrap();
        let rules_after_first =
            fs::read_to_string(td.path().join("docs/architext/data/rules.json")).unwrap();

        let second = repair_instruction_rules(td.path(), false);
        assert!(second.is_empty(), "second run must converge: {second:?}");
        assert_eq!(
            fs::read_to_string(td.path().join("AGENTS.md")).unwrap(),
            agents_after_first
        );
        assert_eq!(
            fs::read_to_string(td.path().join("docs/architext/data/rules.json")).unwrap(),
            rules_after_first
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_write_is_reported_not_swallowed() {
        // T-1 (Rule 13 fail-loud): a repair whose write fails must carry the
        // error — previously `let _ = write_json(...)` reported the change as
        // applied while the file stayed untouched.
        use std::os::unix::fs::PermissionsExt;
        let td = temp_dir();
        write(
            td.path(),
            "docs/architext/data/manifest.json",
            r#"{ "schemaVersion": "1.4.0", "files": {} }"#,
        );
        let manifest_path = td.path().join("docs/architext/data/manifest.json");
        fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o444)).unwrap();

        let outcomes = repair_manifest_data(td.path(), false);

        fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!outcomes.is_empty());
        assert!(
            outcomes.iter().all(|o| o.error.is_some()),
            "write failure must surface: {outcomes:?}"
        );
        let manifest = read_json(&manifest_path).unwrap();
        assert_eq!(manifest["schemaVersion"], "1.4.0", "file untouched on failure");

        // And the DoctorRepair JSON shape marks it failed for serve/CLI callers.
        let repair = DoctorRepair {
            category: "manifest".into(),
            file: "manifest.json".into(),
            summary: outcomes[0].summary.clone(),
            error: outcomes[0].error.clone(),
        };
        assert_eq!(repair.to_json()["status"], "failed");
        assert!(repair.to_json()["error"].as_str().is_some());
    }

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
        assert_eq!(summaries(&changes), vec!["create missing Release Truth history index".to_string()]);

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
        assert_eq!(summaries(&changes), vec!["create missing Release Truth history index".to_string()]);
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
        assert_eq!(summaries(&changes), vec!["create missing Release Truth history index".to_string()]);

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
        assert_eq!(summaries(&changes), vec!["create missing Release Truth history index".to_string()]);

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
            changes.iter().any(|c| c.summary.contains("add v1-0-0 to Release Truth history")),
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
            changes.iter().any(|c| c.summary.contains("normalize incomplete release detail v0-1-0.json")),
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
            changes.iter().any(|c| c.summary.contains("normalize incomplete release detail v0-2-0.json")),
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
        assert!(changes.iter().any(|c| c.summary.contains("normalize incomplete release detail")));

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
    fn backup_file_never_clobbers_an_existing_backup() {
        // QA cp-6 (crit): two backups of the same file in the same second used
        // the same name, and fs::copy silently overwrote the first — destroying
        // the very artifact the backup exists to preserve.
        let td = temp_dir();
        let path = td.path().join("v0-1-0.json");
        fs::write(&path, "first").unwrap();
        let first = backup_file(&path).expect("first backup");
        fs::write(&path, "second").unwrap();
        let second = backup_file(&path).expect("second backup");

        assert_ne!(first, second, "backup names must be unique");
        assert_eq!(fs::read_to_string(td.path().join(&first)).unwrap(), "first");
        assert_eq!(fs::read_to_string(td.path().join(&second)).unwrap(), "second");
    }

    #[test]
    fn corrupt_advice_ledger_is_backed_up_not_clobbered() {
        // AUDIT/QA cp-6: an unparseable repair-advice.json was silently reset,
        // losing every pending reconciliation record without a trace.
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
        write(td.path(), "docs/architext/data/repair-advice.json", "{ corrupt ledger");

        repair_release_truth_data(td.path(), false);

        let data_dir = td.path().join("docs/architext/data");
        let ledger_backups: Vec<String> = std::fs::read_dir(&data_dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| n.starts_with("repair-advice.json.") && n.ends_with(".bak"))
            .collect();
        assert_eq!(ledger_backups.len(), 1, "corrupt ledger must be backed up: {ledger_backups:?}");
        assert_eq!(
            fs::read_to_string(data_dir.join(&ledger_backups[0])).unwrap(),
            "{ corrupt ledger"
        );
        let advice = read_json(&data_dir.join("repair-advice.json")).expect("fresh ledger");
        assert_eq!(advice["advice"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn advice_entries_for_the_same_file_are_superseded() {
        // AUDIT cp-6: repeated recovery of the same file must supersede the
        // stale entry (whose backup reference may be gone), not accumulate.
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
        write(
            td.path(),
            "docs/architext/data/repair-advice.json",
            r#"{ "advice": [
                 { "kind": "release-detail-recovery", "file": "releases/v0-1-0.json",
                   "backup": "releases/v0-1-0.json.20260101T000000Z.bak",
                   "backfilledFields": ["summary"], "instruction": "stale" },
                 { "kind": "release-detail-recovery", "file": "releases/other.json",
                   "backup": "releases/other.json.20260101T000000Z.bak",
                   "backfilledFields": ["summary"], "instruction": "keep me" } ] }"#,
        );

        repair_release_truth_data(td.path(), false);

        let advice =
            read_json(&td.path().join("docs/architext/data/repair-advice.json")).unwrap();
        let entries = advice["advice"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "superseded, not accumulated: {entries:?}");
        let for_file: Vec<&Value> = entries
            .iter()
            .filter(|e| e["file"] == "releases/v0-1-0.json")
            .collect();
        assert_eq!(for_file.len(), 1);
        assert_ne!(for_file[0]["instruction"], "stale", "new entry replaces the stale one");
        assert!(entries.iter().any(|e| e["file"] == "releases/other.json"));
    }

    #[cfg(unix)]
    #[test]
    fn backup_failure_aborts_every_write() {
        // AUDIT cp-6 (high): if the backup cannot be taken, the repair must not
        // overwrite anything — a failed backup followed by a write destroys the
        // only copy of the original, inverting E-1's guarantee.
        use std::os::unix::fs::PermissionsExt;
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
        let releases_dir = td.path().join("docs/architext/data/releases");
        let before = fs::read_to_string(releases_dir.join("v0-1-0.json")).unwrap();
        // Read-only dir: the .bak cannot be created.
        fs::set_permissions(&releases_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let changes = repair_release_truth_data(td.path(), false);

        fs::set_permissions(&releases_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(
            changes[0].summary.contains("backup") && changes[0].summary.contains("failed"),
            "failure must be loud: {changes:?}"
        );
        assert_eq!(
            fs::read_to_string(releases_dir.join("v0-1-0.json")).unwrap(),
            before,
            "original must be untouched"
        );
        assert!(
            !td.path().join("docs/architext/data/repair-advice.json").exists(),
            "no advice for repairs that did not happen"
        );
    }

    #[test]
    fn repair_release_truth_regenerates_unparseable_index() {
        // Status treats an unreadable index the same as a missing one; apply must too.
        let td = temp_dir();
        write_release_truth_target(td.path(), Some("{ not json"));

        let changes = repair_release_truth_data(td.path(), false);
        assert_eq!(summaries(&changes), vec!["create missing Release Truth history index".to_string()]);

        let index = read_json(&td.path().join("docs/architext/data/releases/index.json"))
            .expect("index.json regenerated");
        assert_eq!(index["currentReleaseId"], "v1-0-0");
    }
}
