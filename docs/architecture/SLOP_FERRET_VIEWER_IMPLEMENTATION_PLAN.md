# Slop Ferret viewer integration — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an optional `slop-ferret.json` enrichment to Architext, a `architext slop-ferret` CLI command that produces it, and a new "Slop Ferret" viewer mode that renders the sweep coverage and findings.

**Architecture:** The CLI command consumes slop-ferret's existing `plan.json`, `discharge.json`, and `findings.json`, runs `ferret enumerate` for the accounting, reconstructs the sweep record, and writes `docs/architext/data/slop-ferret.json`. The viewer loads it through the manifest (like `code-graph.json`) and renders a read-only panel.

**Tech Stack:** Rust 2021, Leptos 0.6, serde, jsonschema, standard library subprocess.

## Global Constraints

- Rust edition 2021, resolver 2.
- No Node/npm in the build/test path.
- All schemas live in `viewer/schema/` and are embedded into the native binary with `include_dir`.
- Viewer modes are enumerated in `theme.rs`; every mode must have a label, id, and icon in `mode_icon.rs`.
- The viewer data fetcher treats optional enrichments as non-fatal: a malformed file renders an error panel, never a blank app.
- CLI commands live in `crates/architext-cli/src/commands/` and are wired in `main.rs` and `args.rs`.
- Feature branches from `develop`; PRs merge to `develop`.

---

## File map

| File | Responsibility |
|---|---|
| `crates/architext-cli/src/args.rs` | Add `--plan`, `--discharge`, `--findings` flags and `slop-ferret` to `KNOWN_COMMANDS`. |
| `crates/architext-cli/src/usage.rs` | Add `slop-ferret` to help text. |
| `crates/architext-cli/src/main.rs` | Route `slop-ferret` to the new command. |
| `crates/architext-cli/src/commands/mod.rs` | Declare `pub mod slop_ferret;`. |
| `crates/architext-cli/src/commands/slop_ferret.rs` | The command implementation: parse inputs, run `ferret enumerate`, build and write `slop-ferret.json`, update manifest. |
| `viewer/schema/manifest.schema.json` | Allow `slopFerret` in `manifest.files`. |
| `viewer/schema/slop-ferret.schema.json` | Schema for the new data file. |
| `crates/architext-core/src/validation/schema.rs` | Add `slopFerret` to `FILE_SCHEMAS` (optional). |
| `crates/architext-viewer/src/data/models.rs` | Add `SlopFerret` and `SlopFerretFinding` serde models. |
| `crates/architext-viewer/src/data/fetch.rs` | Load `slopFerret` from manifest into `ArchitectureData`. |
| `crates/architext-viewer/src/theme.rs` | Add `Mode::SlopFerret`. |
| `crates/architext-viewer/src/components/mode_icon.rs` | Add icon path for Slop Ferret. |
| `crates/architext-viewer/src/components/slop_ferret_panel.rs` | New panel component. |
| `crates/architext-viewer/src/components/canvas_panel.rs` | Render `SlopFerretPanel` for the new mode. |
| `crates/architext-viewer/src/components/mod.rs` | Declare `pub mod slop_ferret_panel;`. |
| `crates/architext-viewer/src/styles.css` | Minimal panel styles (reuse existing utility classes where possible). |
| `crates/architext-cli/tests/slop_ferret.rs` | CLI integration test with fixtures. |
| `crates/architext-viewer/src/components/slop_ferret_panel.rs` tests | Native Leptos tests in the same file. |
| `docs/architecture/SLOP_FERRET_VIEWER_DESIGN.md` | Already approved; update if the implementation diverges. |
| `../slop-ferret/README.md` | Document Architext integration. |
| `../slop-ferret/docs/architecture/dataflow.md` | Extend dataflow diagram. |

---

## Task 1: CLI argv parsing

**Files:**
- Modify: `crates/architext-cli/src/args.rs`
- Modify: `crates/architext-cli/src/usage.rs`
- Modify: `crates/architext-cli/src/main.rs`
- Modify: `crates/architext-cli/src/commands/mod.rs`
- Test: `crates/architext-cli/src/args.rs` (existing module tests)

**Interfaces:**
- Consumes: none.
- Produces: `ParsedArgs` gains `plan`, `discharge`, `findings` fields; `slop-ferret` is a recognized command.

- [ ] **Step 1: Add flag fields to `ParsedArgs`**

Add to `crates/architext-cli/src/args.rs`:

```rust
pub struct ParsedArgs {
    // ... existing fields ...
    pub plan: String,
    pub discharge: String,
    pub findings: String,
}
```

- [ ] **Step 2: Add defaults in `parse_args`**

In the `ParsedArgs` initializer, add:

```rust
plan: String::new(),
discharge: String::new(),
findings: String::new(),
```

- [ ] **Step 3: Add `slop-ferret` to `KNOWN_COMMANDS`**

```rust
const KNOWN_COMMANDS: &[&str] = &[
    "install", "upgrade", "sync", "init", "doctor", "status", "serve",
    "validate", "build", "prompt", "skill", "clean", "explain", "help", "version",
    "update", "slop-ferret",
];
```

- [ ] **Step 4: Parse the three flags**

In the `match arg` block, add:

```rust
"--plan" => {
    index += 1;
    opts.plan = rest.get(index).cloned().unwrap_or_default();
}
"--discharge" => {
    index += 1;
    opts.discharge = rest.get(index).cloned().unwrap_or_default();
}
"--findings" => {
    index += 1;
    opts.findings = rest.get(index).cloned().unwrap_or_default();
}
```

- [ ] **Step 5: Add command to usage text**

In `crates/architext-cli/src/usage.rs`, add under Commands:

```text
  slop-ferret [path] --plan <file> --discharge <file> --findings <file>
                             Bundle a slop-ferret sweep into docs/architext/data/slop-ferret.json.
```

Add to Options:

```text
  --plan <file>              slop-ferret plan.json (required for slop-ferret).
  --discharge <file>         slop-ferret discharge.json (required for slop-ferret).
  --findings <file>          slop-ferret findings.json (required for slop-ferret).
```

- [ ] **Step 6: Route in `main.rs`**

Add to the `match opts.command.as_str()` block:

```rust
"slop-ferret" => commands::slop_ferret::run(
    &target,
    &opts.plan,
    &opts.discharge,
    &opts.findings,
),
```

- [ ] **Step 7: Declare module**

In `crates/architext-cli/src/commands/mod.rs`:

```rust
pub mod slop_ferret;
```

- [ ] **Step 8: Add argv test**

In `crates/architext-cli/src/args.rs` tests:

```rust
#[test]
fn slop_ferret_flags_parsed() {
    let opts = parse_args(&args("slop-ferret . --plan plan.json --discharge discharge.json --findings findings.json")).unwrap();
    assert_eq!(opts.command, "slop-ferret");
    assert_eq!(opts.target, ".");
    assert_eq!(opts.plan, "plan.json");
    assert_eq!(opts.discharge, "discharge.json");
    assert_eq!(opts.findings, "findings.json");
}
```

- [ ] **Step 9: Run tests**

```bash
cargo test -p architext-cli args::tests
```

Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add crates/architext-cli/src/args.rs crates/architext-cli/src/usage.rs crates/architext-cli/src/main.rs crates/architext-cli/src/commands/mod.rs
git commit -m "feat(cli): argv parsing for slop-ferret command"
```

---

## Task 2: JSON schema for `slop-ferret.json`

**Files:**
- Create: `viewer/schema/slop-ferret.schema.json`
- Modify: `viewer/schema/manifest.schema.json`

**Interfaces:**
- Consumes: none.
- Produces: a schema file the validator and any external consumer can use.

- [ ] **Step 1: Write the schema**

Create `viewer/schema/slop-ferret.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://architext.local/schema/slop-ferret.schema.json",
  "type": "object",
  "additionalProperties": false,
  "required": [
    "schema", "sha", "date", "attested_repo", "attested_plan",
    "denominator", "accounting", "findings"
  ],
  "properties": {
    "schema": { "type": "integer", "minimum": 1 },
    "origin": { "type": "string" },
    "root_commit": { "type": "string" },
    "identity_method": { "type": "string" },
    "sha": { "type": "string", "minLength": 1 },
    "date": { "type": "string", "minLength": 1 },
    "attested_repo": { "type": "string", "minLength": 1 },
    "attested_plan": { "type": "string", "minLength": 1 },
    "denominator": { "type": "integer", "minimum": 0 },
    "waived": { "type": "integer", "minimum": 0 },
    "worklist_size": { "type": "integer", "minimum": 0 },
    "unmatched_size": { "type": "integer", "minimum": 0 },
    "accounting": { "type": "string", "enum": ["complete", "incomplete"] },
    "vocab_provenance": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    },
    "tier": { "type": "string" },
    "families_not_run": {
      "type": "array",
      "items": { "type": "string", "minLength": 1 }
    },
    "checked_clean": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["class", "method"],
        "properties": {
          "class": { "type": "string", "minLength": 1 },
          "method": { "type": "string", "minLength": 1 }
        }
      }
    },
    "near_misses": {
      "type": "array",
      "items": { "type": "string" }
    },
    "findings_verified": { "type": "integer", "minimum": 0 },
    "findings_suspected": { "type": "integer", "minimum": 0 },
    "report_path": { "type": "string" },
    "findings": {
      "type": "array",
      "items": { "$ref": "#/$defs/finding" }
    }
  },
  "$defs": {
    "finding": {
      "type": "object",
      "additionalProperties": false,
      "required": ["title", "file", "class", "severity", "status"],
      "properties": {
        "title": { "type": "string", "minLength": 1 },
        "file": { "type": "string", "minLength": 1 },
        "class": { "type": "string", "minLength": 1 },
        "severity": { "type": "string", "enum": ["blocking", "fix-or-file", "note"] },
        "status": { "type": "string", "enum": ["VERIFIED", "SUSPECTED"] },
        "claim": { "type": "string" },
        "refutation": { "type": "string" },
        "bar": { "type": "string" },
        "evidence": { "type": "string" },
        "remediation": { "type": "string" },
        "occurrences": { "type": "integer", "minimum": 1 }
      }
    }
  }
}
```

- [ ] **Step 2: Allow `slopFerret` in manifest schema**

In `viewer/schema/manifest.schema.json`, add inside `files.properties`:

```json
"slopFerret": { "type": "string", "minLength": 1 }
```

- [ ] **Step 3: Validate the schemas themselves are JSON**

```bash
python3 -m json.tool viewer/schema/slop-ferret.schema.json > /dev/null
python3 -m json.tool viewer/schema/manifest.schema.json > /dev/null
```

Expected: no output (success).

- [ ] **Step 4: Commit**

```bash
git add viewer/schema/slop-ferret.schema.json viewer/schema/manifest.schema.json
git commit -m "feat(schema): slop-ferret data contract and manifest key"
```

---

## Task 3: Wire schema into validation

**Files:**
- Modify: `crates/architext-core/src/validation/schema.rs`

**Interfaces:**
- Consumes: `viewer/schema/slop-ferret.schema.json`.
- Produces: `FILE_SCHEMAS` now validates `slopFerret` when present.

- [ ] **Step 1: Add entry to `FILE_SCHEMAS`**

After the `codeGraph` entry, add:

```rust
FileSchema { key: "slopFerret", schema_file: "slop-ferret.schema.json", required: false },
```

- [ ] **Step 2: Run core validation tests**

```bash
cargo test -p architext-core
```

Expected: existing tests pass; no regressions.

- [ ] **Step 3: Commit**

```bash
git add crates/architext-core/src/validation/schema.rs
git commit -m "feat(validation): validate optional slop-ferret.json"
```

---

## Task 4: CLI command implementation

**Files:**
- Create: `crates/architext-cli/src/commands/slop_ferret.rs`
- Modify: `crates/architext-cli/Cargo.toml` if new deps are needed (none expected)

**Interfaces:**
- Consumes: `plan.json`, `discharge.json`, `findings.json`, `ferret` binary, git.
- Produces: `docs/architext/data/slop-ferret.json` and updated `manifest.json`.

- [ ] **Step 1: Create the command module**

Create `crates/architext-cli/src/commands/slop_ferret.rs`:

```rust
//! `slop-ferret [path] --plan <file> --discharge <file> --findings <file>`
//!
//! Bundles a slop-ferret sweep into an Architext-readable snapshot:
//!   docs/architext/data/slop-ferret.json
//!
//! The command runs `ferret enumerate` to compute the accounting, then
//! reconstructs the sweep record from the plan, discharge, and git provenance.

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
    #[serde(skip_serializing_if = "String::is_empty")]
    root_commit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    identity_method: String,
    sha: String,
    date: String,
    attested_repo: String,
    attested_plan: String,
    denominator: i64,
    waived: i64,
    worklist_size: i64,
    unmatched_size: i64,
    accounting: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    vocab_provenance: Option<std::collections::BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    tier: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    families_not_run: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    checked_clean: Vec<CheckedClean>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    near_misses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    findings_verified: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    findings_suspected: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    report_path: String,
    findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
struct CheckedClean {
    class: String,
    method: String,
}

#[derive(Debug, Deserialize, Serialize)]
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

/// Run the command. Exits with `ferret enumerate`'s code; writes the snapshot
/// before exiting so partial data is still viewable.
pub fn run(target: &Path, plan: &str, discharge: &str, findings: &str) {
    if plan.is_empty() || discharge.is_empty() || findings.is_empty() {
        eprintln!("Usage: architext slop-ferret [path] --plan <file> --discharge <file> --findings <file>");
        std::process::exit(2);
    }

    let plan_path = PathBuf::from(plan);
    let discharge_path = PathBuf::from(discharge);
    let findings_path = PathBuf::from(findings);

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
        .arg(&plan_path)
        .arg(&discharge_path)
        .arg(target)
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
    std::fs::create_dir_all(&data_dir).ok();

    let snapshot = build_snapshot(target, &plan_doc, &discharge_doc, &result, findings_doc.findings);
    let out_path = data_dir.join("slop-ferret.json");
    let json = serde_json::to_string_pretty(&snapshot).expect("snapshot serializes");
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
    let production_files = plan.get("production_files").and_then(|v| v.as_array()).map(|a| a.len() as i64).unwrap_or(0);
    let h_worklist = plan.get("h_worklist").and_then(|v| v.as_array()).map(|a| a.len() as i64).unwrap_or(0);
    let h_unmatched = plan.get("h_unmatched").and_then(|v| v.as_array()).map(|a| a.len() as i64).unwrap_or(0);

    let (origin, root_commit, identity_method) = repo_identity(target);
    let date = git_commit_date(target, plan_sha(plan));

    Snapshot {
        schema: SLOP_FERRET_SCHEMA,
        origin,
        root_commit,
        identity_method,
        sha: plan_sha(plan).to_string(),
        date,
        attested_repo: result.attested.repo.clone(),
        attested_plan: result.attested.plan.clone(),
        denominator: plan.get("production_total").and_then(|v| v.as_i64()).unwrap_or(production_files),
        waived: result.attested.waived,
        worklist_size: h_worklist,
        unmatched_size: h_unmatched,
        accounting: if result.remaining.is_empty() { "complete".to_string() } else { "incomplete".to_string() },
        vocab_provenance: plan.get("vocab_provenance").and_then(|v| serde_json::from_value(v.clone()).ok()),
        tier: discharge.get("tier").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        families_not_run: string_array(discharge, "families_not_run"),
        checked_clean: discharge
            .get("checked_clean")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let class = item.get("class")?.as_str()?;
                        let method = item.get("method")?.as_str()?;
                        Some(CheckedClean { class: class.to_string(), method: method.to_string() })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        near_misses: string_array(discharge, "near_misses"),
        findings_verified: discharge.get("findings_verified").and_then(|v| v.as_i64()),
        findings_suspected: discharge.get("findings_suspected").and_then(|v| v.as_i64()),
        report_path: discharge.get("report_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
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
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
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

    let abs = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
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
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        _ => Vec::new(),
    }
}

fn strip_git_url(raw: &str) -> String {
    let mut s = raw.trim();
    if s.starts_with("git@") {
        s = &s[4..];
        if let Some((host, path)) = s.split_once(':') {
            return format!("{}/{}", host, path.trim_end_matches(".git"));
        }
    }
    s.trim_start_matches("https://")
        .trim_start_matches("http://")
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

    let files = manifest
        .get_mut("files")
        .and_then(|v| v.as_object_mut())
        .expect("manifest.files is an object");

    if !files.contains_key("slopFerret") {
        files.insert("slopFerret".to_string(), "slop-ferret.json".into());
        let updated = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
        if let Err(e) = std::fs::write(&manifest_path, updated) {
            eprintln!("slop-ferret: warning: failed to update manifest.json: {e}");
        } else {
            println!("slop-ferret: registered slopFerret in manifest.json");
        }
    }
}
```

- [ ] **Step 2: Build the CLI crate**

```bash
cargo build -p architext-cli
```

Expected: compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add crates/architext-cli/src/commands/slop_ferret.rs
git commit -m "feat(cli): slop-ferret bundle command"
```

---

## Task 5: CLI integration test with fixtures

**Files:**
- Create: `crates/architext-cli/tests/fixtures/slop-ferret/plan.json`
- Create: `crates/architext-cli/tests/fixtures/slop-ferret/discharge.json`
- Create: `crates/architext-cli/tests/fixtures/slop-ferret/findings.json`
- Create: `crates/architext-cli/tests/fixtures/slop-ferret/manifest.json`
- Create: `crates/architext-cli/tests/slop_ferret.rs`

**Interfaces:**
- Consumes: the command from Task 4.
- Produces: golden `slop-ferret.json` and passing test.

- [ ] **Step 1: Create minimal fixture plan.json**

`crates/architext-cli/tests/fixtures/slop-ferret/plan.json`:

```json
{
  "contract": "slop-gate/2",
  "sha": "abc1234def5678",
  "fidelity": "rta",
  "reachability_computable": true,
  "map_provenance": {
    "generator": "magma/0.2.0",
    "contract_version": "codemap-rows/1"
  },
  "vocab_provenance": {
    "lexicon": "~/.claude/skills/slop-ferret/references/ai-slop-lexicon.md",
    "lexicon_version": "2026-08-04.1",
    "signals_total": "3",
    "signals_from_lexicon": "3",
    "signals_from_repo": "0"
  },
  "unseeded_families": [],
  "unseeded_detail": {},
  "candidates": [],
  "production_total": 2,
  "production_files": ["main.go", "lib.go"],
  "production_unclassified": [],
  "h_worklist": [
    { "path": "main.go", "reason": "network/untrusted-io" }
  ],
  "h_required": [
    { "path": "main.go", "reason": "network/untrusted-io" }
  ],
  "h_deferred": [],
  "h_unmatched": ["lib.go"],
  "h_unmatched_changes": [],
  "change_baseline": "",
  "instructions": "read every h_required path"
}
```

- [ ] **Step 2: Create discharge.json**

`crates/architext-cli/tests/fixtures/slop-ferret/discharge.json`:

```json
{
  "sha": "abc1234def5678",
  "read_paths": ["main.go", "lib.go"],
  "coverage_waived": [],
  "families_not_run": [],
  "candidates_filed": [],
  "candidates_cleared": [],
  "candidates_refuted": [],
  "tier": "1",
  "checked_clean": [
    { "class": "H · latent defect", "method": "read every h_required path" }
  ],
  "near_misses": [],
  "findings_verified": 1,
  "findings_suspected": 0,
  "report_path": "/tmp/report.html"
}
```

- [ ] **Step 3: Create findings.json**

`crates/architext-cli/tests/fixtures/slop-ferret/findings.json`:

```json
{
  "repo": "github.com/robot-accomplice/example",
  "skill_version": "v0.1.0",
  "families_run": ["H"],
  "findings": [
    {
      "title": "Example finding",
      "file": "main.go",
      "class": "H · latent defect",
      "severity": "blocking",
      "status": "VERIFIED",
      "claim": "Example claim.",
      "refutation": "None found.",
      "bar": "reproduce RED",
      "evidence": "Line 12 calls os/exec.Command with a user string.",
      "remediation": "Validate input.",
      "occurrences": 1
    }
  ]
}
```

- [ ] **Step 4: Create manifest.json**

`crates/architext-cli/tests/fixtures/slop-ferret/manifest.json`:

```json
{
  "schemaVersion": "1.0.0",
  "project": {
    "id": "example",
    "name": "Example",
    "summary": "Fixture project."
  },
  "generatedAt": "2026-08-04T00:00:00Z",
  "defaultViewId": "context",
  "files": {
    "nodes": "nodes.json",
    "flows": "flows.json",
    "views": "views.json",
    "dataClassification": "data-classification.json",
    "decisions": "decisions.json",
    "risks": "risks.json",
    "glossary": "glossary.json"
  },
  "notes": []
}
```

Create empty required data files in the same fixture directory so the command can write into a real `docs/architext/data/` tree.

- [ ] **Step 5: Write the integration test**

`crates/architext-cli/tests/slop_ferret.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../target/debug/architext");
    path
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("slop-ferret")
}

#[test]
fn bundles_slop_ferret_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("docs").join("architext").join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let fixture = fixture_dir();
    std::fs::copy(fixture.join("manifest.json"), data_dir.join("manifest.json")).unwrap();
    for name in ["nodes.json", "flows.json", "views.json", "data-classification.json", "decisions.json", "risks.json", "glossary.json"] {
        std::fs::write(data_dir.join(name), "{\"nodes\":[],\"flows\":[],\"views\":[],\"classes\":[],\"decisions\":[],\"risks\":[],\"terms\":[]}".as_bytes()).ok();
    }

    let output = Command::new(bin())
        .arg("slop-ferret")
        .arg(tmp.path())
        .arg("--plan")
        .arg(fixture.join("plan.json"))
        .arg("--discharge")
        .arg(fixture.join("discharge.json"))
        .arg("--findings")
        .arg(fixture.join("findings.json"))
        .output()
        .expect("architext slop-ferret runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() || output.status.code() == Some(3),
        "unexpected exit: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );

    let snapshot_path = data_dir.join("slop-ferret.json");
    assert!(snapshot_path.exists(), "snapshot not written");

    let text = std::fs::read_to_string(&snapshot_path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["sha"], "abc1234def5678");
    assert_eq!(doc["attested_repo"], "2/2");
    assert_eq!(doc["findings"].as_array().unwrap().len(), 1);

    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(data_dir.join("manifest.json")).unwrap()
    ).unwrap();
    assert_eq!(manifest["files"]["slopFerret"], "slop-ferret.json");
}
```

- [ ] **Step 6: Add `tempfile` dev-dependency if missing**

Check `crates/architext-cli/Cargo.toml` for `tempfile` under `[dev-dependencies]`. If absent, add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 7: Run the test**

```bash
cargo test -p architext-cli --test slop_ferret
```

Expected: test passes.

- [ ] **Step 8: Commit**

```bash
git add crates/architext-cli/tests/
git commit -m "test(cli): slop-ferret bundle command fixtures and integration test"
```

---

## Task 6: Viewer data models and fetcher

**Files:**
- Modify: `crates/architext-viewer/src/data/models.rs`
- Modify: `crates/architext-viewer/src/data/fetch.rs`

**Interfaces:**
- Consumes: `viewer/schema/slop-ferret.schema.json` shape.
- Produces: `ArchitectureData.slop_ferret: Option<Result<SlopFerret, FetchError>>`.

- [ ] **Step 1: Add models**

In `crates/architext-viewer/src/data/models.rs`, after the `CodeGraph` block, add:

```rust
// ─── slop-ferret.json ───────────────────────────────────────────────────────

/// A slop-ferret sweep snapshot consumed by Architext. Optional third-party
/// enrichment, like `CodeGraph`.
#[derive(Debug, Clone, Deserialize)]
pub struct SlopFerret {
    pub schema: i64,
    #[serde(default)]
    pub origin: String,
    #[serde(default, rename = "root_commit")]
    pub root_commit: String,
    #[serde(default, rename = "identity_method")]
    pub identity_method: String,
    pub sha: String,
    pub date: String,
    #[serde(rename = "attested_repo")]
    pub attested_repo: String,
    #[serde(rename = "attested_plan")]
    pub attested_plan: String,
    pub denominator: i64,
    #[serde(default)]
    pub waived: i64,
    #[serde(default, rename = "worklist_size")]
    pub worklist_size: i64,
    #[serde(default, rename = "unmatched_size")]
    pub unmatched_size: i64,
    pub accounting: String,
    #[serde(default, rename = "vocab_provenance")]
    pub vocab_provenance: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub tier: String,
    #[serde(default, rename = "families_not_run")]
    pub families_not_run: Vec<String>,
    #[serde(default, rename = "checked_clean")]
    pub checked_clean: Vec<SlopFerretCheckedClean>,
    #[serde(default, rename = "near_misses")]
    pub near_misses: Vec<String>,
    #[serde(default, rename = "findings_verified")]
    pub findings_verified: Option<i64>,
    #[serde(default, rename = "findings_suspected")]
    pub findings_suspected: Option<i64>,
    #[serde(default, rename = "report_path")]
    pub report_path: String,
    #[serde(default)]
    pub findings: Vec<SlopFerretFinding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlopFerretCheckedClean {
    pub class: String,
    pub method: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlopFerretFinding {
    pub title: String,
    pub file: String,
    pub class: String,
    pub severity: String,
    pub status: String,
    #[serde(default)]
    pub claim: String,
    #[serde(default)]
    pub refutation: String,
    #[serde(default)]
    pub bar: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub remediation: String,
    #[serde(default)]
    pub occurrences: Option<i64>,
}
```

- [ ] **Step 2: Add field to `ArchitectureData`**

In `crates/architext-viewer/src/data/fetch.rs`, add to `ArchitectureData`:

```rust
pub slop_ferret: Option<Result<SlopFerret, FetchError>>,
```

- [ ] **Step 3: Load the file in `load_architecture_data`**

After the `codeGraph` block, add:

```rust
if let Some(url) = data_url(&manifest, "slopFerret") {
    data.slop_ferret = Some(get_json::<SlopFerret>(&url).await);
}
```

- [ ] **Step 4: Build the viewer crate**

```bash
cargo build -p architext-viewer --target wasm32-unknown-unknown
```

Expected: compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add crates/architext-viewer/src/data/models.rs crates/architext-viewer/src/data/fetch.rs
git commit -m "feat(viewer): slop-ferret data models and fetcher"
```

---

## Task 7: Viewer mode and icon

**Files:**
- Modify: `crates/architext-viewer/src/theme.rs`
- Modify: `crates/architext-viewer/src/components/mode_icon.rs`

**Interfaces:**
- Consumes: `Mode` enum pattern.
- Produces: `Mode::SlopFerret` with id `"slop-ferret"`, label `"Slop Ferret"`, and icon.

- [ ] **Step 1: Add mode variant**

In `crates/architext-viewer/src/theme.rs`:

```rust
pub enum Mode {
    // ... existing variants ...
    Rules,
    SlopFerret,
}
```

Update `Mode::ALL` to include `Mode::SlopFerret` after `Rules`.

- [ ] **Step 2: Add label and id**

In `label`:

```rust
Mode::SlopFerret => "Slop Ferret",
```

In `id`:

```rust
Mode::SlopFerret => "slop-ferret",
```

- [ ] **Step 3: Add rail summary**

In `rail_summary`, add:

```rust
Mode::SlopFerret => Some("Review slop-ferret sweep coverage and findings."),
```

- [ ] **Step 4: Add icon path**

In `crates/architext-viewer/src/components/mode_icon.rs`:

```rust
Mode::SlopFerret => "M4 12h3 M4 16h3 M7 12c3 0 2 4 5 4 M7 16c3 0 2-4 5-4 M12 14h7 M16 11l3 3-3 3",
```

(Icon: a stylized ferret/snout with a target line. Replace with a better SVG path if a designer provides one.)

- [ ] **Step 5: Run viewer tests**

```bash
cargo test -p architext-viewer theme::tests
```

Expected: the existing `every_mode_has_a_label_an_id_and_an_icon` test now covers the new variant and passes.

- [ ] **Step 6: Commit**

```bash
git add crates/architext-viewer/src/theme.rs crates/architext-viewer/src/components/mode_icon.rs
git commit -m "feat(viewer): Slop Ferret mode and icon"
```

---

## Task 8: Slop Ferret panel component

**Files:**
- Create: `crates/architext-viewer/src/components/slop_ferret_panel.rs`
- Modify: `crates/architext-viewer/src/components/mod.rs`
- Modify: `crates/architext-viewer/src/components/canvas_panel.rs`
- Modify: `crates/architext-viewer/src/styles.css`

**Interfaces:**
- Consumes: `AppState.data.slop_ferret`.
- Produces: rendered panel with coverage banner and findings list.

- [ ] **Step 1: Create the panel component**

`crates/architext-viewer/src/components/slop_ferret_panel.rs`:

```rust
//! Slop Ferret sweep viewer: coverage banner + findings list.

use leptos::*;

use crate::data::models::{SlopFerret, SlopFerretFinding};
use crate::state::use_app_state;

const SEV_ORDER: &[&str] = &["blocking", "fix-or-file", "note"];

#[component]
pub fn SlopFerretPanel() -> impl IntoView {
    let state = use_app_state();

    let snapshot = move || {
        state
            .data
            .get()
            .slop_ferret
            .as_ref()
            .and_then(|res| res.as_ref().ok().cloned())
    };

    let error = move || {
        state
            .data
            .get()
            .slop_ferret
            .as_ref()
            .and_then(|res| res.as_ref().err().map(|e| e.to_string()))
    };

    view! {
        <div class="slop-ferret-panel">
            {move || {
                if let Some(err) = error() {
                    return view! {
                        <div class="slop-ferret-panel__error">
                            <span class="overline">"SLOP FERRET"</span>
                            <p>{err}</p>
                        </div>
                    }.into_view();
                }
                match snapshot() {
                    Some(doc) => render_snapshot(&doc),
                    None => view! {
                        <div class="slop-ferret-panel__empty">
                            <span class="overline">"SLOP FERRET"</span>
                            <p>"No slop-ferret snapshot is registered in this project. Run `architext slop-ferret --plan ... --discharge ... --findings ...` to create one."</p>
                        </div>
                    }.into_view(),
                }
            }}
        </div>
    }
}

fn render_snapshot(doc: &SlopFerret) -> View {
    let mut findings = doc.findings.clone();
    findings.sort_by(|a, b| {
        let rank = |s: &str| SEV_ORDER.iter().position(|&r| r == s.to_lowercase().as_str()).unwrap_or(SEV_ORDER.len());
        let ra = rank(&a.severity);
        let rb = rank(&b.severity);
        if ra != rb {
            return ra.cmp(&rb);
        }
        let verified_a = a.status == "VERIFIED";
        let verified_b = b.status == "VERIFIED";
        if verified_a != verified_b {
            return if verified_a { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        a.title.cmp(&b.title)
    });

    view! {
        <div class="slop-ferret-panel__content">
            <div class="slop-ferret-panel__header">
                <span class="overline">"SLOP FERRET"</span>
                <h2>"Sweep findings"</h2>
            </div>
            <div class="slop-ferret-panel__banner">
                <div class="slop-ferret-panel__metric">
                    <span class="slop-ferret-panel__value">{doc.attested_repo.clone()}</span>
                    <span class="slop-ferret-panel__label">"repo read"</span>
                </div>
                <div class="slop-ferret-panel__metric">
                    <span class="slop-ferret-panel__value">{doc.attested_plan.clone()}</span>
                    <span class="slop-ferret-panel__label">"plan dispositioned"</span>
                </div>
                <div class="slop-ferret-panel__metric">
                    <span class="slop-ferret-panel__value">{doc.denominator.to_string()}</span>
                    <span class="slop-ferret-panel__label">"files"</span>
                </div>
                <div class="slop-ferret-panel__metric">
                    <span class={move || format!("slop-ferret-panel__badge slop-ferret-panel__badge--{}", doc.accounting.clone())}>
                        {doc.accounting.clone()}
                    </span>
                    <span class="slop-ferret-panel__label">"accounting"</span>
                </div>
            </div>
            <div class="slop-ferret-panel__findings">
                {findings.into_iter().map(finding_card).collect_view()}
            </div>
        </div>
    }
    .into_view()
}

fn finding_card(f: SlopFerretFinding) -> View {
    let sev_class = format!("slop-ferret-panel__severity slop-ferret-panel__severity--{}", f.severity.to_lowercase());
    let status_class = format!("slop-ferret-panel__status slop-ferret-panel__status--{}", f.status.to_lowercase());
    view! {
        <div class="slop-ferret-panel__finding">
            <div class="slop-ferret-panel__finding-header">
                <span class=sev_class>{f.severity.clone()}</span>
                <span class=status_class>{f.status.clone()}</span>
                <span class="slop-ferret-panel__class">{f.class.clone()}</span>
            </div>
            <h3 class="slop-ferret-panel__finding-title">{f.title}</h3>
            <div class="slop-ferret-panel__meta">{f.file.clone()}</div>
            {(!f.claim.is_empty()).then(|| view! {
                <p class="slop-ferret-panel__claim">{f.claim.clone()}</p>
            })}
            {(!f.evidence.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"EVIDENCE"</span>
                    <p>{f.evidence.clone()}</p>
                </div>
            })}
            {(!f.remediation.is_empty()).then(|| view! {
                <div class="slop-ferret-panel__section">
                    <span class="overline">"REMEDIATION"</span>
                    <p>{f.remediation.clone()}</p>
                </div>
            })}
        </div>
    }
    .into_view()
}
```

- [ ] **Step 2: Declare module**

In `crates/architext-viewer/src/components/mod.rs`, add:

```rust
pub mod slop_ferret_panel;
```

- [ ] **Step 3: Wire into canvas panel**

In `crates/architext-viewer/src/components/canvas_panel.rs`:

Import:

```rust
use crate::components::slop_ferret_panel::SlopFerretPanel;
```

Add to the `None => match state.mode.get()` arm:

```rust
Mode::SlopFerret => view! { <SlopFerretPanel/> }.into_view(),
```

- [ ] **Step 4: Add minimal CSS**

In `crates/architext-viewer/src/styles.css`, append:

```css
.slop-ferret-panel {
  height: 100%;
  overflow: auto;
  padding: var(--space-lg);
}

.slop-ferret-panel__content {
  max-width: 720px;
  margin: 0 auto;
}

.slop-ferret-panel__banner {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-md);
  margin: var(--space-lg) 0;
  padding: var(--space-md);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--panel);
}

.slop-ferret-panel__metric {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-xs);
}

.slop-ferret-panel__value {
  font-size: var(--text-xl);
  font-weight: 600;
}

.slop-ferret-panel__badge--complete {
  color: var(--ok);
}

.slop-ferret-panel__badge--incomplete {
  color: var(--warn);
}

.slop-ferret-panel__findings {
  display: flex;
  flex-direction: column;
  gap: var(--space-md);
}

.slop-ferret-panel__finding {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: var(--space-md);
  background: var(--panel);
}

.slop-ferret-panel__finding-header {
  display: flex;
  gap: var(--space-sm);
  margin-bottom: var(--space-sm);
}

.slop-ferret-panel__severity,
.slop-ferret-panel__status,
.slop-ferret-panel__class {
  font-size: var(--text-xs);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  padding: 2px 6px;
  border-radius: var(--radius-sm);
}

.slop-ferret-panel__severity--blocking { background: var(--sev-high); color: var(--sev-high-text); }
.slop-ferret-panel__severity--fix-or-file { background: var(--sev-medium); color: var(--sev-medium-text); }
.slop-ferret-panel__severity--note { background: var(--sev-low); color: var(--sev-low-text); }

.slop-ferret-panel__status--verified { border: 1px solid var(--ok); color: var(--ok); }
.slop-ferret-panel__status--suspected { border: 1px dashed var(--warn); color: var(--warn); }

.slop-ferret-panel__finding-title {
  margin: 0 0 var(--space-xs);
  font-size: var(--text-lg);
}

.slop-ferret-panel__meta {
  font-size: var(--text-sm);
  color: var(--muted);
  margin-bottom: var(--space-sm);
}

.slop-ferret-panel__claim {
  font-style: italic;
  margin-bottom: var(--space-md);
}

.slop-ferret-panel__section {
  margin-top: var(--space-md);
}

.slop-ferret-panel__error,
.slop-ferret-panel__empty {
  padding: var(--space-lg);
  text-align: center;
  color: var(--muted);
}
```

(Use existing CSS variables; adjust class names if the variable set differs.)

- [ ] **Step 5: Build viewer**

```bash
trunk build --release --config crates/architext-viewer/Trunk.toml
```

Expected: compiles cleanly.

- [ ] **Step 6: Commit**

```bash
git add crates/architext-viewer/src/components/slop_ferret_panel.rs crates/architext-viewer/src/components/mod.rs crates/architext-viewer/src/components/canvas_panel.rs crates/architext-viewer/src/styles.css
git commit -m "feat(viewer): Slop Ferret panel"
```

---

## Task 9: Viewer panel tests

**Files:**
- Modify: `crates/architext-viewer/src/components/slop_ferret_panel.rs`

**Interfaces:**
- Consumes: `SlopFerretPanel` component.
- Produces: passing native Leptos tests.

- [ ] **Step 1: Add tests at the bottom of the panel file**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{SlopFerret, SlopFerretFinding};

    fn sample_finding(title: &str, severity: &str, status: &str) -> SlopFerretFinding {
        SlopFerretFinding {
            title: title.to_string(),
            file: "main.go".to_string(),
            class: "H · latent defect".to_string(),
            severity: severity.to_string(),
            status: status.to_string(),
            claim: "claim".to_string(),
            refutation: String::new(),
            bar: String::new(),
            evidence: String::new(),
            remediation: String::new(),
            occurrences: None,
        }
    }

    #[test]
    fn severity_sorts_blocking_first() {
        let mut findings = vec![
            sample_finding("Cosmetic", "note", "VERIFIED"),
            sample_finding("Auth bug", "blocking", "VERIFIED"),
            sample_finding("Dupe", "fix-or-file", "VERIFIED"),
        ];
        findings.sort_by(|a, b| {
            let rank = |s: &str| match s {
                "blocking" => 0,
                "fix-or-file" => 1,
                "note" => 2,
                _ => 3,
            };
            rank(&a.severity).cmp(&rank(&b.severity))
        });
        assert_eq!(findings[0].title, "Auth bug");
        assert_eq!(findings[1].title, "Dupe");
        assert_eq!(findings[2].title, "Cosmetic");
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p architext-viewer slop_ferret_panel
```

Expected: tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/architext-viewer/src/components/slop_ferret_panel.rs
git commit -m "test(viewer): slop-ferret panel severity sorting"
```

---

## Task 10: End-to-end validation

**Files:**
- Create: `test/fixtures/slop-ferret-viewer/` with data files

**Interfaces:**
- Consumes: all prior tasks.
- Produces: confirmed `architext validate .` passes.

- [ ] **Step 1: Create a fixture project**

Under `test/fixtures/slop-ferret-viewer/docs/architext/data/`, create:
- `manifest.json` with `slopFerret: "slop-ferret.json"`
- `slop-ferret.json` (valid, with one finding)
- Minimal required data files (`nodes.json`, `flows.json`, etc.)

- [ ] **Step 2: Run validation**

```bash
cargo run -p architext-cli -- validate test/fixtures/slop-ferret-viewer
```

Expected: "Architext validation passed."

- [ ] **Step 3: Commit**

```bash
git add test/fixtures/slop-ferret-viewer/
git commit -m "test(fixtures): slop-ferret viewer validation fixture"
```

---

## Task 11: Update slop-ferret documentation

**Files (in ../slop-ferret repo):**
- Modify: `../slop-ferret/README.md`
- Modify: `../slop-ferret/docs/architecture/dataflow.md`

**Interfaces:**
- Consumes: approved design doc.
- Produces: prose telling users the sweep output can be viewed in Architext.

- [ ] **Step 1: Create feature branch in slop-ferret repo**

```bash
cd ../slop-ferret
git checkout -b feature/architext-docs develop
```

- [ ] **Step 2: Update README.md**

In the "Symbiosis: magma, architext, slop-ferret" section, after the diagram, add:

```markdown
**Viewing the sweep in Architext.** After running `ferret enumerate`, point Architext at the sweep artifacts:

```bash
architext slop-ferret . --plan plan.json --discharge discharge.json --findings findings.json
architext serve .
```

Architext writes `docs/architext/data/slop-ferret.json` and renders the sweep coverage and findings as a "Slop Ferret" mode alongside the architecture data and Magma code graph.
```

- [ ] **Step 3: Update docs/architecture/dataflow.md**

Extend the dataflow diagram to show the Architext consume path. Add a box for `architext slop-ferret` between `ferret enumerate` and `docs/architext/data/slop-ferret.json`, and an arrow from there to `architext serve` / `architext-viewer`.

- [ ] **Step 4: Commit and push**

```bash
git add README.md docs/architecture/dataflow.md
git commit -m "docs: Architext integration for slop-ferret output"
GH_TOKEN=$(gh auth token --user robot-accomplice) git push origin feature/architext-docs
```

- [ ] **Step 5: Report the PR URL**

The push output will include a PR URL; note it for the final summary.

---

## Task 12: Final verification and PR preparation

**Files:**
- All of the above.

- [ ] **Step 1: Run full Rust checks**

```bash
cargo test --workspace
cargo test -p architext-routing --test corpus_fitness
cargo run -p architext-cli -- validate .
trunk build --release --config crates/architext-viewer/Trunk.toml
```

Expected: all pass.

- [ ] **Step 2: Push the architext branch**

```bash
cd /Users/jmachen/code/architext
GH_TOKEN=$(gh auth token --user robot-accomplice) git push origin feature/slop-ferret-viewer
```

- [ ] **Step 3: Open PRs**

Open PRs from:
- `robot-accomplice/architext:feature/slop-ferret-viewer` → `develop`
- `robot-accomplice/slop-ferret:feature/architext-docs` → `develop`

If `gh pr create` is permitted, use:

```bash
gh pr create --repo robot-accomplice/architext --base develop --head feature/slop-ferret-viewer --title "feat(viewer): Slop Ferret sweep visualization" --body "See docs/architecture/SLOP_FERRET_VIEWER_DESIGN.md for the approved design."
```

(Otherwise, provide the URLs for manual creation.)

---

## Self-review checklist

- [ ] Spec coverage: every section of `SLOP_FERRET_VIEWER_DESIGN.md` maps to at least one task above.
- [ ] No placeholders: no TBD, TODO, or vague "add validation" steps.
- [ ] Type consistency: `SlopFerret` field names in the viewer model match the schema and the CLI output.
- [ ] Testability: every task ends with a command or test assertion.
- [ ] Git workflow: feature branches from `develop`, PRs to `develop`.
