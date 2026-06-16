//! Verbose status printing for the sync command.
//!
//! The sync command calls `printStatus(finalStatus, { verbose: true })` after
//! performing writes. The non-sync `status` command uses non-verbose mode.
//! This module adds the verbose sections (instruction files, root scripts)
//! on top of the base `format_status_lines` from commands::status.

use serde_json::Value;

use crate::commands::status::format_status_lines;

/// Format verbose status lines as used by sync's final `printStatus(status, { verbose: true })`.
pub fn format_verbose_status_lines(status: &Value) -> Vec<String> {
    let mut lines = format_status_lines(status);

    // verbose: doctor repairs details
    let repairs = status["doctorRepairs"].as_array();
    if let Some(repairs) = repairs {
        if !repairs.is_empty() {
            lines.push("Doctor repairs available:".to_string());
            for repair in repairs {
                if let Some(summary) = repair["summary"].as_str() {
                    lines.push(format!("- {summary}"));
                }
            }
        }
    }

    // verbose: C4 issues
    if let Some(c4) = status.get("c4").filter(|v| !v.is_null()) {
        let issues = c4["issues"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
        if !issues.is_empty() {
            lines.push("C4 issues:".to_string());
            for issue in issues {
                if let Some(s) = issue.as_str() {
                    lines.push(format!("- {s}"));
                }
            }
        }
        let remaining = c4["remainingIssues"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
        if !remaining.is_empty() {
            lines.push("C4 issues requiring manual architecture judgment:".to_string());
            for issue in remaining {
                if let Some(s) = issue.as_str() {
                    lines.push(format!("- {s}"));
                }
            }
        }
        let drilldown = c4["drilldownIssues"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
        if !drilldown.is_empty() {
            lines.push("C4 drilldown gaps requiring architecture documentation:".to_string());
            for issue in drilldown {
                if let Some(s) = issue.as_str() {
                    lines.push(format!("- {s}"));
                }
            }
        }
    }

    // verbose: instruction rule migration details
    if let Some(ir) = status.get("instructionRules").filter(|v| !v.is_null()) {
        let candidates = ir["candidateRules"].as_array().map(|a| a.len()).unwrap_or(0);
        let rewrites = ir["rewriteFiles"].as_array().map(|a| a.len()).unwrap_or(0);
        let ambiguous = ir["ambiguousFiles"].as_array().map(|a| a.len()).unwrap_or(0);
        if candidates > 0 || rewrites > 0 || ambiguous > 0 {
            lines.push("Instruction rule migration:".to_string());
            for rule in ir["candidateRules"].as_array().into_iter().flatten() {
                if let Some(title) = rule["title"].as_str() {
                    lines.push(format!("- Candidate rule: {title}"));
                }
            }
            for file in ir["rewriteFiles"].as_array().into_iter().flatten() {
                if let Some(path) = file["path"].as_str() {
                    lines.push(format!("- Rewrite pointer: {path}"));
                }
            }
            for file in ir["ambiguousFiles"].as_array().into_iter().flatten() {
                if let Some(path) = file["path"].as_str() {
                    let reason = file["reason"].as_str().unwrap_or("");
                    lines.push(format!("- Ambiguous content preserved: {path} ({reason})"));
                }
            }
        }
    }

    // verbose: instruction files
    if let Some(instr_status) = status.get("instructionStatus").and_then(|v| v.as_object()) {
        lines.push("Instruction files:".to_string());
        // Keep order of INSTRUCTION_FILES
        for file_name in super::target_layout::INSTRUCTION_FILES {
            if let Some(file_status) = instr_status.get(*file_name) {
                let state = if file_status["hasArchitextSection"].as_bool().unwrap_or(false) {
                    if file_status["mentionsCopiedTemplate"].as_bool().unwrap_or(false) {
                        "outdated Architext section"
                    } else {
                        "current Architext section"
                    }
                } else if file_status["exists"].as_bool().unwrap_or(false) {
                    "missing Architext section"
                } else {
                    "missing"
                };
                lines.push(format!("- {file_name}: {state}"));
            }
        }
    }

    // verbose: root scripts
    if let Some(root_scripts) = status.get("rootScripts").and_then(|v| v.as_object()) {
        lines.push("Root scripts:".to_string());
        for &(name, _expected) in super::target_layout::ROOT_SCRIPTS {
            if let Some(script) = root_scripts.get(name) {
                let present = script["present"].as_bool().unwrap_or(false);
                let recommended = script["recommended"].as_bool().unwrap_or(false);
                let state = if present {
                    if recommended { "ok" } else { "custom" }
                } else {
                    "missing"
                };
                lines.push(format!("- {name}: {state}"));
            }
        }
    }

    lines
}
