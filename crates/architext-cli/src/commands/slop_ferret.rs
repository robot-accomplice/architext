//! `slop-ferret [path] --plan <file> --discharge <file> --findings <file>`
//!
//! Bundles a slop-ferret sweep into an Architext-readable snapshot:
//!   docs/architext/data/slop-ferret.json
//!
//! The command runs `ferret enumerate` to compute the accounting, then
//! reconstructs the sweep record from the plan, discharge, git provenance, and
//! the authored findings.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

const SLOP_FERRET_SCHEMA: i64 = 1;

/// Sweep record fields derived from the plan/discharge/git.
#[derive(Debug, Serialize)]
struct Snapshot {
    schema: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    origin: String,
    #[serde(skip_serializing_if = "String::is_empty", rename = "root_commit")]
    root_commit: String,
    #[serde(skip_serializing_if = "String::is_empty", rename = "identity_method")]
    identity_method: String,
    sha: String,
    date: String,
    #[serde(rename = "attested_repo")]
    attested_repo: String,
    #[serde(rename = "attested_plan")]
    attested_plan: String,
    denominator: i64,
    #[serde(skip_serializing_if = "is_zero")]
    waived: i64,
    #[serde(skip_serializing_if = "is_zero", rename = "worklist_size")]
    worklist_size: i64,
    #[serde(skip_serializing_if = "is_zero", rename = "unmatched_size")]
    unmatched_size: i64,
    accounting: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "vocab_provenance")]
    vocab_provenance: Option<std::collections::BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    tier: String,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "families_not_run")]
    families_not_run: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "checked_clean")]
    checked_clean: Vec<CheckedClean>,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "near_misses")]
    near_misses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "findings_verified")]
    findings_verified: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "findings_suspected")]
    findings_suspected: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty", rename = "report_path")]
    report_path: String,
    findings: Vec<Finding>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

#[derive(Debug, Serialize)]
struct CheckedClean {
    class: String,
    method: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Finding {
    title: String,
    file: String,
    class: String,
    severity: String,
    status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    claim: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    refutation: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    bar: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    evidence: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    remediation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    occurrences: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AuthoredFindings {
    #[serde(default)]
    findings: Vec<Finding>,
}

#[derive(Debug, Deserialize)]
struct ResultJson {
    attested: AttestedJson,
    #[serde(default)]
    remaining: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AttestedJson {
    repo: String,
    plan: String,
    #[serde(default)]
    waived: i64,
}

/// Run the command. Exits with `ferret enumerate`'s code; writes the snapshot
/// before exiting so partial data is still viewable.
pub fn run(target: &Path, plan: &str, discharge: &str, findings: &str) {
    if plan.is_empty() || discharge.is_empty() || findings.is_empty() {
        eprintln!("Usage: architext slop-ferret [path] --plan <file> --discharge <file> --findings <file>");
        std::process::exit(2);
    }

    let plan_path = resolve_input(target, plan);
    let discharge_path = resolve_input(target, discharge);
    let findings_path = resolve_input(target, findings);

    for (label, path) in [("plan", &plan_path), ("discharge", &discharge_path), ("findings", &findings_path)] {
        if !path.is_file() {
            eprintln!("slop-ferret: {label} file not found: {}", path.display());
            std::process::exit(2);
        }
    }

    if Command::new("ferret").arg("--version").output().is_err() {
        eprintln!("slop-ferret: `ferret` binary not found on PATH. Install from https://github.com/robot-accomplice/slop-ferret");
        std::process::exit(2);
    }

    let enumerate_output = Command::new("ferret")
        .arg("enumerate")
        .arg("--no-record")
        .arg(&plan_path)
        .arg(&discharge_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let (enum_stdout, enum_stderr, enum_code) = match enumerate_output {
        Ok(out) => (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            out.status.code().unwrap_or(2),
        ),
        Err(e) => {
            eprintln!("slop-ferret: failed to run `ferret enumerate`: {e}");
            std::process::exit(2);
        }
    };

    if enum_code == 4 {
        eprintln!("slop-ferret: `ferret enumerate` refused the sweep:");
        if !enum_stderr.is_empty() {
            eprintln!("{enum_stderr}");
        } else {
            eprintln!("{enum_stdout}");
        }
        std::process::exit(4);
    }

    let plan_doc: serde_json::Value = read_json_file(&plan_path, "plan");
    let discharge_doc: serde_json::Value = read_json_file(&discharge_path, "discharge");
    let findings_doc: AuthoredFindings = read_json_file_typed(&findings_path, "findings");

    let result: ResultJson = match serde_json::from_str(&enum_stdout) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("slop-ferret: could not parse `ferret enumerate` output as JSON: {e}");
            eprintln!("stdout was:\n{enum_stdout}");
            std::process::exit(2);
        }
    };

    let data_dir = target.join("docs").join("architext").join("data");
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("slop-ferret: cannot create {}: {e}", data_dir.display());
        std::process::exit(2);
    }

    let snapshot = build_snapshot(target, &plan_doc, &discharge_doc, &result, findings_doc.findings);
    let out_path = data_dir.join("slop-ferret.json");
    let json = match serde_json::to_string_pretty(&snapshot) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("slop-ferret: failed to serialize snapshot: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::write(&out_path, json) {
        eprintln!("slop-ferret: failed to write {}: {e}", out_path.display());
        std::process::exit(2);
    }
    println!("slop-ferret: wrote {}", out_path.display());

    update_manifest(&data_dir);

    if enum_code == 3 {
        eprintln!("slop-ferret: `ferret enumerate` reported open items:");
        for item in &result.remaining {
            eprintln!("  - {item}");
        }
    }

    std::process::exit(enum_code);
}

fn resolve_input(target: &Path, raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() {
        p
    } else {
        target.join(p)
    }
}

fn read_json_file(path: &Path, label: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("slop-ferret: cannot read {label} file {}: {e}", path.display());
        std::process::exit(2);
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("slop-ferret: invalid JSON in {label} file {}: {e}", path.display());
        std::process::exit(2);
    })
}

fn read_json_file_typed<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("slop-ferret: cannot read {label} file {}: {e}", path.display());
        std::process::exit(2);
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("slop-ferret: invalid JSON in {label} file {}: {e}", path.display());
        std::process::exit(2);
    })
}

fn build_snapshot(
    target: &Path,
    plan: &serde_json::Value,
    discharge: &serde_json::Value,
    result: &ResultJson,
    findings: Vec<Finding>,
) -> Snapshot {
    let production_files_len = plan
        .get("production_files")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as i64)
        .unwrap_or(0);
    let h_worklist_len = plan
        .get("h_worklist")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as i64)
        .unwrap_or(0);
    let h_unmatched_len = plan
        .get("h_unmatched")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as i64)
        .unwrap_or(0);

    let (origin, root_commit, identity_method) = repo_identity(target);
    let sha = plan_sha(plan);
    let date = git_commit_date(target, sha);

    Snapshot {
        schema: SLOP_FERRET_SCHEMA,
        origin,
        root_commit,
        identity_method,
        sha: sha.to_string(),
        date,
        attested_repo: result.attested.repo.clone(),
        attested_plan: result.attested.plan.clone(),
        denominator: plan
            .get("production_total")
            .and_then(|v| v.as_i64())
            .unwrap_or(production_files_len),
        waived: result.attested.waived,
        worklist_size: h_worklist_len,
        unmatched_size: h_unmatched_len,
        accounting: if result.remaining.is_empty() {
            "complete".to_string()
        } else {
            "incomplete".to_string()
        },
        vocab_provenance: plan
            .get("vocab_provenance")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        tier: discharge
            .get("tier")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        families_not_run: string_array(discharge, "families_not_run"),
        checked_clean: discharge
            .get("checked_clean")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let class = item.get("class")?.as_str()?;
                        let method = item.get("method")?.as_str()?;
                        Some(CheckedClean {
                            class: class.to_string(),
                            method: method.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        near_misses: string_array(discharge, "near_misses"),
        findings_verified: discharge.get("findings_verified").and_then(|v| v.as_i64()),
        findings_suspected: discharge.get("findings_suspected").and_then(|v| v.as_i64()),
        report_path: discharge
            .get("report_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        findings,
    }
}

fn plan_sha(plan: &serde_json::Value) -> &str {
    plan.get("sha").and_then(|v| v.as_str()).unwrap_or("")
}

fn string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn repo_identity(target: &Path) -> (String, String, String) {
    let origin = git_line(target, &["remote", "get-url", "origin"]);
    let origin = strip_git_url(&origin);

    let roots = git_lines(target, &["rev-list", "--max-parents=0", "--reverse", "HEAD"]);
    if !roots.is_empty() {
        let shallow = git_line(target, &["rev-parse", "--is-shallow-repository"]);
        if shallow != "true" {
            return (origin, roots.join(","), "root-commit".to_string());
        }
    }

    (origin, String::new(), "absolute-path".to_string())
}

fn git_commit_date(target: &Path, sha: &str) -> String {
    if sha.is_empty() {
        return String::new();
    }
    git_line(target, &["show", "-s", "--format=%cs", sha])
}

fn git_line(target: &Path, args: &[&str]) -> String {
    git_lines(target, args).into_iter().next().unwrap_or_default()
}

fn git_lines(target: &Path, args: &[&str]) -> Vec<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(target).args(args);
    match cmd.output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn strip_git_url(raw: &str) -> String {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("{}/{}", host, path.trim_end_matches(".git"));
        }
    }
    s.strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s)
        .trim_end_matches(".git")
        .to_string()
}

fn update_manifest(data_dir: &Path) {
    let manifest_path = data_dir.join("manifest.json");
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("slop-ferret: warning: cannot read manifest.json: {e}");
            return;
        }
    };
    let mut manifest: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("slop-ferret: warning: manifest.json is invalid JSON: {e}");
            return;
        }
    };

    let files = match manifest.get_mut("files").and_then(|v| v.as_object_mut()) {
        Some(f) => f,
        None => {
            eprintln!("slop-ferret: warning: manifest.files is missing or not an object");
            return;
        }
    };

    if !files.contains_key("slopFerret") {
        files.insert("slopFerret".to_string(), "slop-ferret.json".into());
        match serde_json::to_string_pretty(&manifest) {
            Ok(updated) => {
                if let Err(e) = std::fs::write(&manifest_path, updated) {
                    eprintln!("slop-ferret: warning: failed to update manifest.json: {e}");
                } else {
                    println!("slop-ferret: registered slopFerret in manifest.json");
                }
            }
            Err(e) => eprintln!("slop-ferret: warning: failed to serialize manifest.json: {e}"),
        }
    }
}
