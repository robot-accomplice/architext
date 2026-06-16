//! `status [path] [--json]` — port of the JS `status` command handler
//! (~line 1558 of architext-cli.mjs) and `printStatus` (terminal-presenter.mjs).

use std::path::Path;
use std::process;

use architext_core::status::collect_status;

/// Format a status `serde_json::Value` into the lines that `printStatus`
/// (terminal-presenter.mjs / `statusLines`) produces.
///
/// The JS `statusLines` function is ported here verbatim.
pub fn format_status_lines(status: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::new();

    macro_rules! push {
        ($fmt:literal $(, $arg:expr)*) => {
            lines.push(format!($fmt $(, $arg)*));
        };
    }

    push!("Target: {}", status["target"].as_str().unwrap_or(""));
    push!(
        "Architext data: {}",
        if status["installed"].as_bool().unwrap_or(false) { "installed" } else { "missing" }
    );
    push!("CLI: {}", status["cliVersion"].as_str().unwrap_or(""));
    push!(
        "Copied install: {}",
        if status["copiedInstallDetected"].as_bool().unwrap_or(false) { "detected" } else { "no" }
    );

    // Gitignore
    let gitignore_missing = status["gitignoreMissing"].as_array().map(|a| a.len()).unwrap_or(0);
    if gitignore_missing > 0 {
        let items: Vec<&str> = status["gitignoreMissing"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        push!("Gitignore: missing {}", items.join(", "));
    } else {
        push!("Gitignore: ok");
    }

    // Tracked generated
    let tracked_count = status["trackedGenerated"].as_array().map(|a| a.len()).unwrap_or(0);
    if tracked_count > 0 {
        push!("Generated artifacts tracked: {tracked_count}");
    } else {
        push!("Generated artifacts tracked: none");
    }

    // C4
    if let Some(c4) = status.get("c4").filter(|v| !v.is_null()) {
        let issue_count = c4["issues"].as_array().map(|a| a.len()).unwrap_or(0);
        if issue_count > 0 {
            let plural = if issue_count == 1 { "" } else { "s" };
            push!("C4 documents: {issue_count} issue{plural}");
        } else {
            push!("C4 documents: ok");
        }
        let drilldown_count = c4["drilldownIssues"].as_array().map(|a| a.len()).unwrap_or(0);
        if drilldown_count > 0 {
            let plural = if drilldown_count == 1 { "" } else { "s" };
            push!("C4 drilldown: {drilldown_count} gap{plural}");
        }
    }

    // Release Truth
    if let Some(rt) = status.get("releaseTruth").filter(|v| !v.is_null()) {
        let configured = rt["configured"].as_bool().unwrap_or(false);
        let index_exists = rt["indexExists"].as_bool().unwrap_or(false);
        let label = if configured && index_exists {
            "configured"
        } else if configured {
            "index missing"
        } else {
            "not configured"
        };
        push!("Release Truth: {label}");
    }

    // Instruction rules
    if let Some(ir) = status.get("instructionRules").filter(|v| !v.is_null()) {
        let count = ir["candidateRules"].as_array().map(|a| a.len()).unwrap_or(0);
        if count > 0 {
            let plural = if count == 1 { "" } else { "s" };
            push!("Instruction rule migration: {count} candidate rule{plural}");
        } else {
            push!("Instruction rule migration: none");
        }
    }

    // Manifest / schema
    if let Some(mf) = status.get("manifest").filter(|v| !v.is_null()) {
        let schema_version = mf["schemaVersion"].as_str().unwrap_or("");
        let repair_count = mf["repairChanges"].as_array().map(|a| a.len()).unwrap_or(0);
        if repair_count > 0 {
            let expected = mf["expectedSchemaVersion"].as_str().unwrap_or("");
            push!("Schema: {schema_version} (expected {expected})");
        } else {
            push!("Schema: {}", if schema_version.is_empty() { "missing" } else { schema_version });
        }
        let pending = mf["migrationPlan"]["pending"].as_array().map(|a| a.len()).unwrap_or(0);
        if pending > 0 {
            push!("Schema migrations: {pending} pending");
        }
    }

    // Doctor repairs
    let repairs_count = status["doctorRepairs"].as_array().map(|a| a.len()).unwrap_or(0);
    if repairs_count > 0 {
        push!("Doctor repairs: {repairs_count}");
    } else {
        push!("Doctor repairs: none");
    }

    // Validation (only if present)
    if let Some(val) = status.get("validation").filter(|v| !v.is_null()) {
        let ok = val["ok"].as_bool().unwrap_or(false);
        push!("Validation: {}", if ok { "passed" } else { "failed" });
    }

    lines
}

/// Verbose variant of `format_status_lines` — port of `statusLines(status,
/// { verbose: true })`. The non-verbose lines are computed by
/// `format_status_lines`; this appends the verbose-only tail and the verbose
/// insertions ("Doctor repairs available:", validation output, C4 issues,
/// instruction-rule migration, instruction files, root scripts).
///
/// To keep parity exact we reproduce the JS line ORDER, which interleaves a few
/// verbose-only lines into the base list (doctor-repairs detail right after the
/// "Doctor repairs:" count; validation output after the "Validation:" line).
pub fn format_status_lines_verbose(status: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::new();

    macro_rules! push {
        ($fmt:literal $(, $arg:expr)*) => { lines.push(format!($fmt $(, $arg)*)); };
    }

    push!("Target: {}", status["target"].as_str().unwrap_or(""));
    push!("Architext data: {}", if status["installed"].as_bool().unwrap_or(false) { "installed" } else { "missing" });
    push!("CLI: {}", status["cliVersion"].as_str().unwrap_or(""));
    push!("Copied install: {}", if status["copiedInstallDetected"].as_bool().unwrap_or(false) { "detected" } else { "no" });

    let gitignore_missing = status["gitignoreMissing"].as_array().map(|a| a.len()).unwrap_or(0);
    if gitignore_missing > 0 {
        let items: Vec<&str> = status["gitignoreMissing"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        push!("Gitignore: missing {}", items.join(", "));
    } else {
        push!("Gitignore: ok");
    }
    let tracked_count = status["trackedGenerated"].as_array().map(|a| a.len()).unwrap_or(0);
    if tracked_count > 0 {
        push!("Generated artifacts tracked: {tracked_count}");
    } else {
        push!("Generated artifacts tracked: none");
    }

    if let Some(c4) = status.get("c4").filter(|v| !v.is_null()) {
        let issue_count = c4["issues"].as_array().map(|a| a.len()).unwrap_or(0);
        if issue_count > 0 {
            let plural = if issue_count == 1 { "" } else { "s" };
            push!("C4 documents: {issue_count} issue{plural}");
        } else {
            push!("C4 documents: ok");
        }
        let drilldown_count = c4["drilldownIssues"].as_array().map(|a| a.len()).unwrap_or(0);
        if drilldown_count > 0 {
            let plural = if drilldown_count == 1 { "" } else { "s" };
            push!("C4 drilldown: {drilldown_count} gap{plural}");
        }
    }
    if let Some(rt) = status.get("releaseTruth").filter(|v| !v.is_null()) {
        let configured = rt["configured"].as_bool().unwrap_or(false);
        let index_exists = rt["indexExists"].as_bool().unwrap_or(false);
        let label = if configured && index_exists { "configured" } else if configured { "index missing" } else { "not configured" };
        push!("Release Truth: {label}");
    }
    if let Some(ir) = status.get("instructionRules").filter(|v| !v.is_null()) {
        let count = ir["candidateRules"].as_array().map(|a| a.len()).unwrap_or(0);
        if count > 0 {
            let plural = if count == 1 { "" } else { "s" };
            push!("Instruction rule migration: {count} candidate rule{plural}");
        } else {
            push!("Instruction rule migration: none");
        }
    }
    if let Some(mf) = status.get("manifest").filter(|v| !v.is_null()) {
        let schema_version = mf["schemaVersion"].as_str().unwrap_or("");
        let repair_count = mf["repairChanges"].as_array().map(|a| a.len()).unwrap_or(0);
        if repair_count > 0 {
            let expected = mf["expectedSchemaVersion"].as_str().unwrap_or("");
            push!("Schema: {schema_version} (expected {expected})");
        } else {
            push!("Schema: {}", if schema_version.is_empty() { "missing" } else { schema_version });
        }
        let pending = mf["migrationPlan"]["pending"].as_array().map(|a| a.len()).unwrap_or(0);
        if pending > 0 {
            push!("Schema migrations: {pending} pending");
        }
    }

    let repairs = status["doctorRepairs"].as_array().cloned().unwrap_or_default();
    if !repairs.is_empty() {
        push!("Doctor repairs: {}", repairs.len());
    } else {
        push!("Doctor repairs: none");
    }
    // verbose: doctor-repairs detail
    if !repairs.is_empty() {
        push!("Doctor repairs available:");
        for repair in &repairs {
            push!("- {}", repair["summary"].as_str().unwrap_or(""));
        }
    }

    if let Some(val) = status.get("validation").filter(|v| !v.is_null()) {
        let ok = val["ok"].as_bool().unwrap_or(false);
        push!("Validation: {}", if ok { "passed" } else { "failed" });
        // verbose always prints validation.output (JS: !ok || verbose)
        if let Some(out) = val["output"].as_str() {
            push!("{out}");
        }
    }

    // verbose tail
    if let Some(c4) = status.get("c4").filter(|v| !v.is_null()) {
        if c4["issues"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            push!("C4 issues:");
            for issue in c4["issues"].as_array().unwrap() {
                push!("- {}", issue.as_str().unwrap_or(""));
            }
        }
        if c4["remainingIssues"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            push!("C4 issues requiring manual architecture judgment:");
            for issue in c4["remainingIssues"].as_array().unwrap() {
                push!("- {}", issue.as_str().unwrap_or(""));
            }
        }
        if c4["drilldownIssues"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            push!("C4 drilldown gaps requiring architecture documentation:");
            for issue in c4["drilldownIssues"].as_array().unwrap() {
                push!("- {}", issue.as_str().unwrap_or(""));
            }
        }
    }

    if let Some(ir) = status.get("instructionRules").filter(|v| !v.is_null()) {
        let candidates = ir["candidateRules"].as_array().cloned().unwrap_or_default();
        let rewrites = ir["rewriteFiles"].as_array().cloned().unwrap_or_default();
        let ambiguous = ir["ambiguousFiles"].as_array().cloned().unwrap_or_default();
        if !candidates.is_empty() || !rewrites.is_empty() || !ambiguous.is_empty() {
            push!("Instruction rule migration:");
            for rule in &candidates {
                push!("- Candidate rule: {}", rule["title"].as_str().unwrap_or(""));
            }
            for file in &rewrites {
                push!("- Rewrite pointer: {}", file["path"].as_str().unwrap_or(""));
            }
            for file in &ambiguous {
                push!("- Ambiguous content preserved: {} ({})", file["path"].as_str().unwrap_or(""), file["reason"].as_str().unwrap_or(""));
            }
        }
    }

    // Instruction files (object iteration order = insertion order; core builds
    // it from the fixed INSTRUCTION_FILES list, matching JS).
    push!("Instruction files:");
    if let Some(obj) = status["instructionStatus"].as_object() {
        for (name, fs) in obj {
            let state = if fs["hasArchitextSection"].as_bool().unwrap_or(false) {
                if fs["mentionsCopiedTemplate"].as_bool().unwrap_or(false) { "outdated Architext section" } else { "current Architext section" }
            } else if fs["exists"].as_bool().unwrap_or(false) {
                "missing Architext section"
            } else {
                "missing"
            };
            push!("- {name}: {state}");
        }
    }
    push!("Root scripts:");
    if let Some(obj) = status["rootScripts"].as_object() {
        for (name, script) in obj {
            let state = if script["present"].as_bool().unwrap_or(false) {
                if script["recommended"].as_bool().unwrap_or(false) { "ok" } else { "custom" }
            } else {
                "missing"
            };
            push!("- {name}: {state}");
        }
    }

    lines
}

/// Print the human-readable status — matches JS `printStatus(status)` in the
/// non-verbose path (the `status` command never passes `verbose:true`).
fn print_status(status: &serde_json::Value) {
    for line in format_status_lines(status) {
        println!("{line}");
    }
}

/// Run `status [--json]`.
pub fn run(target: &Path, json: bool, version: &str) {
    if !target.is_dir() {
        eprintln!("Target is not a directory: {}", target.display());
        process::exit(1);
    }

    // collect_status signature: (target, version, run_validation)
    // The JS `status` handler does NOT run validation (runValidation:false).
    let status = collect_status(target, version, false);

    if json {
        // JS: console.log(JSON.stringify(status, null, 2))
        println!("{}", serde_json::to_string_pretty(&status).unwrap());
    } else {
        print_status(&status);
        let installed = status["installed"].as_bool().unwrap_or(false);
        let needs_migration = status["needsMigration"].as_bool().unwrap_or(false);
        let repair_count = status["doctorRepairs"].as_array().map(|a| a.len()).unwrap_or(0);
        if !installed || needs_migration {
            println!("Next: architext sync");
        } else if repair_count > 0 {
            println!("Next: architext doctor");
        } else {
            println!("Next: architext serve");
        }
    }
}
