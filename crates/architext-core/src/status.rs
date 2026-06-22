//! Status collection orchestrator — Rust port of `collectStatus` and its
//! transitive dependencies from `src/adapters/cli/architext-cli.mjs`.
//!
//! I/O lives here (fs + git); pure domain logic is composed from:
//!   - `domain::c4_quality::{build_node_map, c4_issues_for_view, c4_drilldown_issues, repair_c4_views}`
//!   - `domain::schema_migration::schema_migration_plan`
//!   - `domain::release::{generated_release_index, release_index_generation_changes}`
//!   - `domain::instruction_rules::{planned_instruction_rule_migration, INSTRUCTION_RULE_FILES}`
//!   - `validate_data_dir` (Rust validator)
//!
//! The public entry point is `collect_status`, which returns a `serde_json::Value`
//! whose shape matches the JS `collectStatus` output exactly (minus env-dependent
//! fields normalised by the parity gate).

use std::path::Path;
use std::process::Command;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::domain::{c4_quality, instruction_rules, release, schema_migration};

// ─── Constants (mirrors target-layout.mjs) ───────────────────────────────────

const DATA_SCHEMA_VERSION: &str = "1.5.0";

const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
const GENERATED_IGNORES: &[&str] = &["docs/architext/dist/", "docs/architext/.architext-write.lock/"];

// copiedInstallEntries (relative to docs/architext/)
const COPIED_INSTALL_ENTRIES: &[&str] = &[
    "AGENTS_APPENDIX.md",
    "LLM_ARCHITEXT.md",
    "README.md",
    "index.html",
    "dist",
    "node_modules",
    "package-lock.json",
    "package.json",
    "public",
    "schema",
    "src",
    "tools",
    "tsconfig.json",
    "vite.config.ts",
];

// ─── Path helpers ─────────────────────────────────────────────────────────────

fn architext_dir(target: &Path) -> std::path::PathBuf {
    target.join("docs").join("architext")
}

fn data_dir(target: &Path) -> std::path::PathBuf {
    architext_dir(target).join("data")
}

fn metadata_path(target: &Path) -> std::path::PathBuf {
    architext_dir(target).join(".architext.json")
}

fn legacy_metadata_path(target: &Path) -> std::path::PathBuf {
    architext_dir(target).join(".architext-install.json")
}

fn copied_install_candidate_paths(target: &Path) -> Vec<std::path::PathBuf> {
    COPIED_INSTALL_ENTRIES
        .iter()
        .map(|entry| architext_dir(target).join(entry))
        .collect()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn read_json_file(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_text_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// JS `copiedInstallPaths(target)` — existing copied-install candidate paths,
/// relative to target. Returns `[]` when target IS the package root.
/// For our purposes the package root check is: target resolves to the repo root
/// where `package.json` lives AND the `viewer/` subdir is present. Practically,
/// we skip that check (the parity harness never targets the package root) and
/// just list present candidate paths. The packageSelf guard is replicated via
/// the caller in `collect_status`.
fn copied_install_paths(target: &Path) -> Vec<String> {
    copied_install_candidate_paths(target)
        .into_iter()
        .filter(|p| p.exists())
        .filter_map(|p| p.strip_prefix(target).ok().map(|r| r.to_string_lossy().to_string()))
        .collect()
}

/// Read `.architext.json` (current) falling back to `.architext-install.json` (legacy).
fn read_metadata(target: &Path) -> Option<Value> {
    let current = metadata_path(target);
    let legacy = legacy_metadata_path(target);
    if current.exists() {
        return read_json_file(&current);
    }
    if legacy.exists() {
        return read_json_file(&legacy);
    }
    None
}

/// Run `git rev-parse --is-inside-work-tree` in target dir.
fn git_available(target: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `git ls-files docs/architext/dist` in target dir; returns filtered lines.
fn git_ls_files_dist(target: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "docs/architext/dist"])
        .current_dir(target)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        }
        _ => vec![],
    }
}

// ─── packageJsonInfo ──────────────────────────────────────────────────────────

fn package_json_info(target: &Path) -> (bool, Option<Value>) {
    let file = target.join("package.json");
    if !file.exists() {
        return (false, None);
    }
    (true, read_json_file(&file))
}

// ─── collectC4Status ─────────────────────────────────────────────────────────

fn collect_c4_status(target: &Path) -> Value {
    let target_data_dir = data_dir(target);
    let views_path = target_data_dir.join("views.json");
    let nodes_path = target_data_dir.join("nodes.json");

    if !views_path.exists() || !nodes_path.exists() {
        return json!({ "available": false, "issues": [], "repairChanges": [], "remainingIssues": [] });
    }

    let views_document = match read_json_file(&views_path) {
        Some(v) => v,
        None => return json!({ "available": false, "issues": [], "repairChanges": [], "remainingIssues": [] }),
    };
    let nodes_document = match read_json_file(&nodes_path) {
        Some(v) => v,
        None => return json!({ "available": false, "issues": [], "repairChanges": [], "remainingIssues": [] }),
    };

    let nodes_arr = nodes_document["nodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let node_map = c4_quality::build_node_map(nodes_arr);

    let views_arr = views_document["views"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);

    // issues: flatMap over c4-type views
    let issues: Vec<Value> = views_arr
        .iter()
        .filter(|v| v["type"].as_str().map(|t| t.starts_with("c4-")).unwrap_or(false))
        .flat_map(|v| c4_quality::c4_issues_for_view(v, &node_map))
        .map(Value::String)
        .collect();

    let drilldown_issues: Vec<Value> = c4_quality::c4_drilldown_issues(views_arr, &node_map)
        .into_iter()
        .map(Value::String)
        .collect();

    let repaired = c4_quality::repair_c4_views(views_arr, &node_map);
    let repaired_views = repaired["views"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);

    let remaining_issues: Vec<Value> = repaired_views
        .iter()
        .filter(|v| v["type"].as_str().map(|t| t.starts_with("c4-")).unwrap_or(false))
        .flat_map(|v| c4_quality::c4_issues_for_view(v, &node_map))
        .map(Value::String)
        .collect();

    let repair_changes = repaired["changes"].clone();

    json!({
        "available": true,
        "issues": issues,
        "drilldownIssues": drilldown_issues,
        "repairChanges": repair_changes,
        "remainingIssues": remaining_issues
    })
}

// ─── collectReleaseTruthStatus ────────────────────────────────────────────────

/// Port of `generatedReleaseHistoryChanges(indexPath, indexExists)`.
fn generated_release_history_changes(index_path: &Path, index_exists: bool) -> Vec<String> {
    if !index_exists {
        return vec!["create missing Release Truth history index".to_string()];
    }
    let index = match read_json_file(index_path) {
        Some(v) => v,
        None => return vec!["create missing Release Truth history index".to_string()],
    };
    let release_dir = index_path.parent().unwrap_or(index_path);
    let detail_entries = build_release_detail_entries(release_dir, &index);
    let generated = release::generated_release_index(&index, &detail_entries);
    let changes_val = release::release_index_generation_changes(&index, &generated);
    // changes_val is a JSON array of strings
    changes_val
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// Build `detailEntries` Value array from index.releases → load each detail file.
fn build_release_detail_entries(release_dir: &Path, index: &Value) -> Value {
    let releases = match index["releases"].as_array() {
        Some(arr) => arr,
        None => return json!([]),
    };
    let entries: Vec<Value> = releases
        .iter()
        .filter_map(|summary| {
            let file = summary["file"].as_str()?;
            let detail = read_json_file(&release_dir.join(file))?;
            Some(json!({ "file": file, "detail": detail }))
        })
        .collect();
    Value::Array(entries)
}

fn collect_release_truth_status(target: &Path) -> Option<Value> {
    let manifest_path = data_dir(target).join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    let manifest = read_json_file(&manifest_path)?;
    let configured = manifest["files"]["releases"].is_string();
    let index_path = if configured {
        data_dir(target).join(manifest["files"]["releases"].as_str().unwrap())
    } else {
        data_dir(target).join("releases").join("index.json")
    };
    let index_exists = index_path.exists();
    let repair_changes: Vec<Value> = if configured {
        generated_release_history_changes(&index_path, index_exists)
            .into_iter()
            .map(Value::String)
            .collect()
    } else {
        vec![Value::String("add starter Release Truth data and manifest.files.releases".to_string())]
    };

    Some(json!({
        "configured": configured,
        "indexPath": index_path.to_string_lossy(),
        "indexExists": index_exists,
        "repairChanges": repair_changes
    }))
}

// ─── collectManifestStatus ────────────────────────────────────────────────────

fn collect_manifest_status(target: &Path) -> Option<Value> {
    let manifest_path = data_dir(target).join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    let manifest = read_json_file(&manifest_path)?;
    let current_schema_version = manifest["schemaVersion"].as_str().unwrap_or("").to_string();
    let migration_plan = schema_migration::schema_migration_plan(&current_schema_version, DATA_SCHEMA_VERSION);
    let repair_changes: Vec<Value> = migration_plan["pending"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|m| m["summary"].as_str().map(|s| Value::String(s.to_string()))).collect())
        .unwrap_or_default();

    Some(json!({
        "path": manifest_path.to_string_lossy(),
        "schemaVersion": current_schema_version,
        "expectedSchemaVersion": DATA_SCHEMA_VERSION,
        "migrationPlan": migration_plan,
        "repairChanges": repair_changes
    }))
}

// ─── collectInstructionRuleStatus ────────────────────────────────────────────

/// Port of `cursorRuleFilePaths(target)`.
fn cursor_rule_file_paths(target: &Path) -> Vec<std::path::PathBuf> {
    let cursor_rules_dir = target.join(".cursor").join("rules");
    if !cursor_rules_dir.exists() {
        return vec![];
    }
    let entries = match std::fs::read_dir(&cursor_rules_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    let re = Regex::new(r"\.(md|mdc|txt)$").unwrap();
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| re.is_match(e.file_name().to_string_lossy().as_ref()))
        .map(|e| e.path())
        .collect()
}

/// Port of `instructionRuleSourceFiles(target)`.
fn instruction_rule_source_files(target: &Path) -> Vec<Value> {
    let explicit: Vec<std::path::PathBuf> = instruction_rules::INSTRUCTION_RULE_FILES
        .iter()
        .map(|name| target.join(name))
        .collect();
    let cursor = cursor_rule_file_paths(target);
    let mut files = Vec::new();
    for abs_path in explicit.into_iter().chain(cursor) {
        if !abs_path.exists() {
            continue;
        }
        let text = read_text_file(&abs_path);
        let rel = abs_path.strip_prefix(target).ok().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        files.push(json!({
            "path": rel,
            "absolutePath": abs_path.to_string_lossy(),
            "text": text
        }));
    }
    files
}

fn collect_instruction_rule_status(target: &Path) -> Option<Value> {
    let rules_path = data_dir(target).join("rules.json");
    if !rules_path.exists() {
        return None;
    }
    let rules_document = read_json_file(&rules_path).unwrap_or_else(|| json!({ "rules": [] }));
    let existing_rules = rules_document["rules"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let files = instruction_rule_source_files(target);
    Some(instruction_rules::planned_instruction_rule_migration(&files, existing_rules))
}

// ─── doctorRepairsForStatus ───────────────────────────────────────────────────

/// Port of `doctorRepairsForStatus(status)` from `doctor-repairs.mjs`.
fn doctor_repairs_for_status(status: &Value) -> Value {
    let mut repairs: Vec<Value> = Vec::new();

    // manifest
    for change in status["manifest"]["repairChanges"].as_array().into_iter().flatten() {
        if let Some(s) = change.as_str() {
            repairs.push(json!({
                "id": format!("manifest:{s}"),
                "category": "manifest",
                "file": "docs/architext/data/manifest.json",
                "summary": s
            }));
        }
    }

    // c4
    for change in status["c4"]["repairChanges"].as_array().into_iter().flatten() {
        if let Some(s) = change.as_str() {
            repairs.push(json!({
                "id": format!("c4:{s}"),
                "category": "c4",
                "file": "docs/architext/data/views.json",
                "summary": s
            }));
        }
    }

    // releaseTruth
    for change in status["releaseTruth"]["repairChanges"].as_array().into_iter().flatten() {
        if let Some(s) = change.as_str() {
            repairs.push(json!({
                "id": format!("release-truth:{s}"),
                "category": "release-truth",
                "file": "docs/architext/data/releases/index.json",
                "summary": s
            }));
        }
    }

    // instructionRules
    for change in status["instructionRules"]["repairChanges"].as_array().into_iter().flatten() {
        if let Some(s) = change.as_str() {
            repairs.push(json!({
                "id": format!("instruction-rules:{s}"),
                "category": "instruction-rules",
                "file": "docs/architext/data/rules.json",
                "summary": s
            }));
        }
    }

    Value::Array(repairs)
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Port of `collectStatus(target, version, { runValidation })`.
///
/// Returns a `serde_json::Value` with the same shape as the JS version.
/// `run_validation = false` mirrors the default JS call from `status` command.
pub fn collect_status(target: &Path, version: &str, run_validation: bool) -> Value {
    let target_data_dir = data_dir(target);
    let manifest_path = target_data_dir.join("manifest.json");

    // packageSelf: detect if target IS the package root (the architext npm package
    // itself). We check for the presence of viewer/schema/ which only exists in
    // the real package root. If true, copiedInstallDetected is always false.
    let package_self = target.join("viewer").join("schema").exists()
        && target.join("crates").exists();

    let copied_paths: Vec<String> = if package_self {
        vec![]
    } else {
        copied_install_paths(target)
    };

    let metadata = read_metadata(target).unwrap_or(Value::Null);
    let installed = manifest_path.exists();

    // validation: only run when requested
    let validation: Value = if run_validation {
        let schema_dir = target.join("viewer").join("schema");
        // Try the repo-relative schema dir first; fall back to env var or skip
        let schema_dir = if schema_dir.exists() {
            schema_dir
        } else if let Ok(s) = std::env::var("ARCHITEXT_SCHEMA_DIR") {
            std::path::PathBuf::from(s)
        } else {
            // Find schema relative to the binary
            let fallback = std::path::PathBuf::from("viewer/schema");
            if fallback.exists() { fallback } else { schema_dir }
        };
        if manifest_path.exists() && schema_dir.exists() {
            let outcome = crate::validate_data_dir(&target_data_dir, &schema_dir);
            json!({ "ok": outcome.ok })
        } else if !manifest_path.exists() {
            json!({ "ok": false })
        } else {
            json!({ "ok": null })
        }
    } else {
        Value::Null
    };

    let c4 = if installed { collect_c4_status(target) } else { Value::Null };
    let release_truth = if installed { collect_release_truth_status(target).unwrap_or(Value::Null) } else { Value::Null };
    let manifest_status = if installed { collect_manifest_status(target).unwrap_or(Value::Null) } else { Value::Null };
    let instruction_rules = if installed { collect_instruction_rule_status(target).unwrap_or(Value::Null) } else { Value::Null };

    // gitignoreMissing
    let gitignore_text = read_text_file(&target.join(".gitignore"));
    let gitignore_lines: Vec<&str> = gitignore_text.split(['\n', '\r']).filter(|l| !l.is_empty()).collect();
    let gitignore_missing: Vec<Value> = GENERATED_IGNORES
        .iter()
        .filter(|entry| !gitignore_lines.contains(entry))
        .map(|e| Value::String(e.to_string()))
        .collect();

    // instructionStatus
    let mut instruction_status = Map::new();
    let re_copied = Regex::new(r"docs/architext/(src|schema|tools|package\.json|node_modules)|npm run validate|cd docs/architext").unwrap();
    for &file_name in INSTRUCTION_FILES {
        let file_path = target.join(file_name);
        let text = if file_path.exists() { read_text_file(&file_path) } else { String::new() };
        let exists = file_path.exists();
        let has_architext_section = text.contains("## Architext Architecture Documentation");
        let mentions_copied_template = re_copied.is_match(&text);
        instruction_status.insert(file_name.to_string(), json!({
            "exists": exists,
            "hasArchitextSection": has_architext_section,
            "mentionsCopiedTemplate": mentions_copied_template
        }));
    }

    // rootPackageExists
    let (pkg_exists, _pkg_json) = package_json_info(target);

    // trackedGenerated (git ls-files docs/architext/dist)
    let tracked_generated: Vec<Value> = if git_available(target) {
        git_ls_files_dist(target).into_iter().map(Value::String).collect()
    } else {
        vec![]
    };

    // copiedInstallDetected / needsMigration
    let legacy_meta_exists = legacy_metadata_path(target).exists();
    let copied_install_detected = !package_self && (!copied_paths.is_empty() || legacy_meta_exists);
    let needs_migration = !package_self && (!copied_paths.is_empty() || legacy_meta_exists);

    let mut status = json!({
        "target": target.to_string_lossy(),
        "cliVersion": version,
        "installed": installed,
        "dataDir": target_data_dir.to_string_lossy(),
        "metadata": metadata,
        "copiedInstallDetected": copied_install_detected,
        "copiedInstallPaths": copied_paths,
        "needsMigration": needs_migration,
        "gitignoreMissing": gitignore_missing,
        "instructionStatus": instruction_status,
        "rootPackageExists": pkg_exists,
        "trackedGenerated": tracked_generated,
        "manifest": manifest_status,
        "instructionRules": instruction_rules,
        "c4": c4,
        "releaseTruth": release_truth,
        "validation": validation
    });

    let doctor_repairs = doctor_repairs_for_status(&status);
    status["doctorRepairs"] = doctor_repairs;

    status
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ─── gitignore-missing diff ────────────────────────────────────────────────

    #[test]
    fn gitignore_missing_all_when_absent() {
        let td = temp_dir();
        let status = collect_status(td.path(), "0.0.0", false);
        let missing = status["gitignoreMissing"].as_array().unwrap();
        assert_eq!(missing.len(), GENERATED_IGNORES.len());
        for (i, entry) in GENERATED_IGNORES.iter().enumerate() {
            assert_eq!(missing[i].as_str().unwrap(), *entry);
        }
    }

    #[test]
    fn gitignore_missing_none_when_all_present() {
        let td = temp_dir();
        let content = GENERATED_IGNORES.join("\n") + "\n";
        fs::write(td.path().join(".gitignore"), content).unwrap();
        let status = collect_status(td.path(), "0.0.0", false);
        let missing = status["gitignoreMissing"].as_array().unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn gitignore_missing_partial() {
        let td = temp_dir();
        // Include only the first entry
        let content = format!("{}\n", GENERATED_IGNORES[0]);
        fs::write(td.path().join(".gitignore"), content).unwrap();
        let status = collect_status(td.path(), "0.0.0", false);
        let missing = status["gitignoreMissing"].as_array().unwrap();
        assert_eq!(missing.len(), GENERATED_IGNORES.len() - 1);
    }

    // ─── copied-template regex ─────────────────────────────────────────────────

    #[test]
    fn copied_template_regex_detects_docs_architext_src() {
        let td = temp_dir();
        let content = "# AGENTS\nSee docs/architext/src for details.\n";
        fs::write(td.path().join("AGENTS.md"), content).unwrap();
        let status = collect_status(td.path(), "0.0.0", false);
        assert_eq!(
            status["instructionStatus"]["AGENTS.md"]["mentionsCopiedTemplate"],
            Value::Bool(true)
        );
    }

    #[test]
    fn copied_template_regex_false_for_clean_file() {
        let td = temp_dir();
        let content = "# AGENTS\n## Architext Architecture Documentation\nSome content.\n";
        fs::write(td.path().join("AGENTS.md"), content).unwrap();
        let status = collect_status(td.path(), "0.0.0", false);
        assert_eq!(
            status["instructionStatus"]["AGENTS.md"]["mentionsCopiedTemplate"],
            Value::Bool(false)
        );
    }

    // ─── doctorRepairsForStatus combinator ────────────────────────────────────

    #[test]
    fn doctor_repairs_empty_when_nothing_installed() {
        let td = temp_dir();
        let status = collect_status(td.path(), "0.0.0", false);
        let repairs = status["doctorRepairs"].as_array().unwrap();
        assert!(repairs.is_empty(), "expected no repairs for non-installed dir; got {:?}", repairs);
    }

    #[test]
    fn doctor_repairs_for_status_combinator() {
        // Build a synthetic status object with known repairChanges
        let status = json!({
            "manifest": { "repairChanges": ["update schemaVersion"] },
            "c4": { "repairChanges": ["add scopeNodeId to view-1"] },
            "releaseTruth": { "repairChanges": [] },
            "instructionRules": { "repairChanges": ["add rule foo"] }
        });
        let repairs = doctor_repairs_for_status(&status);
        let arr = repairs.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["category"], "manifest");
        assert_eq!(arr[0]["id"], "manifest:update schemaVersion");
        assert_eq!(arr[0]["file"], "docs/architext/data/manifest.json");
        assert_eq!(arr[1]["category"], "c4");
        assert_eq!(arr[1]["id"], "c4:add scopeNodeId to view-1");
        assert_eq!(arr[2]["category"], "instruction-rules");
        assert_eq!(arr[2]["id"], "instruction-rules:add rule foo");
    }

    // ─── not-installed dir ────────────────────────────────────────────────────

    #[test]
    fn not_installed_returns_expected_shape() {
        let td = temp_dir();
        let status = collect_status(td.path(), "1.0.0", false);
        assert_eq!(status["installed"], Value::Bool(false));
        assert_eq!(status["c4"], Value::Null);
        assert_eq!(status["releaseTruth"], Value::Null);
        assert_eq!(status["manifest"], Value::Null);
        assert_eq!(status["instructionRules"], Value::Null);
        assert_eq!(status["cliVersion"], Value::String("1.0.0".to_string()));
        assert_eq!(status["copiedInstallDetected"], Value::Bool(false));
    }

    // ─── copied-install detection ─────────────────────────────────────────────

    #[test]
    fn copied_install_detected_via_legacy_metadata() {
        let td = temp_dir();
        let legacy = td.path().join("docs").join("architext").join(".architext-install.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}\n").unwrap();
        let status = collect_status(td.path(), "0.0.0", false);
        assert_eq!(status["copiedInstallDetected"], Value::Bool(true));
        assert_eq!(status["needsMigration"], Value::Bool(true));
    }
}
