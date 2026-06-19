//! `prompt [path] [--mode <mode>]` — port of `printPrompt` in
//! `src/adapters/cli/architext-cli.mjs` (~line 991).

use std::path::Path;

const VALID_MODES: &[&str] = &[
    "initial-buildout",
    "architecture-change",
    "repair-validation",
    "source-extraction",
];

fn lead_for_mode(mode: &str, project_name: &str) -> String {
    match mode {
        "initial-buildout" => format!(
            "Build out Architext for {project_name}. Replace neutral starter data with source-backed architecture facts."
        ),
        "architecture-change" => format!(
            "Update Architext for the architecture changes just made in {project_name}. Keep existing stable IDs where concepts already exist."
        ),
        "repair-validation" => format!(
            "Repair Architext JSON validation failures for {project_name}. Do not change application code for this task."
        ),
        "source-extraction" => format!(
            "Inspect {project_name} source files and draft proposed Architext data changes. Do not apply the draft silently."
        ),
        _ => unreachable!("mode already validated"),
    }
}

pub fn run(target: &Path, mode: &str) {
    let data_dir = target.join("docs").join("architext").join("data");
    let manifest_path = data_dir.join("manifest.json");
    let project_name = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["project"]["name"].as_str().map(String::from))
            .unwrap_or_else(|| {
                target
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string()
            })
    } else {
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string()
    };

    // Fallback to initial-buildout if mode is unknown (matches JS behaviour)
    let prompt_mode = if VALID_MODES.contains(&mode) { mode } else { "initial-buildout" };
    let lead = lead_for_mode(prompt_mode, &project_name);

    println!(
        "{lead}

First read AGENTS.md/CLAUDE.md if present, then docs/architext/data/**/*.json.

Rules:
- Update only docs/architext/data/**/*.json unless the Architext package itself is being changed.
- Treat manifest.schemaVersion as the Architext data schema contract version, not the installed CLI/package version. Update it only when the data contract changes or when architext doctor/sync applies a schema repair.
- Reuse stable IDs, create nodes before references, keep flows ordered, and prefer source-path-backed claims.
- Keep flow diagrams free of orphaned elements; every rendered node, edge, marker, and label must be traceable to the selected flow, a selected supporting relationship, or an explicit context relationship shown in the projection. Remove disconnected context, connect it with a labeled relationship, or split it into a separate view.
- Prefer semantic iconography over UML/code diagrams or broad flowchart shape palettes for flow enrichment; mark decision, start, stop, async, persistence, artifact, return, and process semantics with step.kind when the flow needs them. For decision branches, create at least two outgoing outcome steps from the decision node, set step.outcome for each branch label, and expect those branch lines to share the decision step number.
- For sequence diagrams, create explicit return paths; mark returns with kind: \"return\" and returnOf when they answer a specific outbound step, and use sequenceFrames for loops, retries, optional branches, and transaction or consistency blocks that group outbound plus return messages.
- Keep Release Truth data current when release scope, blockers, milestones, evidence, target dates, dependencies, or posture changes.
- Treat Release Truth as reviewed release state, not a planning scratchpad: update detail files for completed, deferred, blocked, reprioritized, or newly scoped work, then refresh the generated release index from those facts.
- Keep Release Path labels concise; put rationale, blocker explanation, evidence, dependencies, and next actions in detail data for the selected release item.
- Use docs/architext/data/roadmap.json for release planning source items. Selected roadmap scope uses source: \"roadmap\"; manually entered scope uses source: \"ad-hoc\" and must be promoted into roadmap.json when approved.
- Use docs/architext/data/rules.json for project rules. Rule categories are maintainer-defined classifications, not a fixed Architext taxonomy. Respect edit/delete protection and rank rules by criticality and order instead of alphabetizing them.
- Build C4 drilldown chains with explicit scopeNodeId metadata for decomposable Context, Container, and Component nodes; leave actors and external dependencies without child views.
- For source extraction, return a reviewable draft of proposed JSON changes with source paths and confidence notes before editing data files. Validation remains required after any accepted edit.
- Mark uncertainty and known gaps explicitly.
- Do not edit copied viewer, schema, package, Vite, or local tool files in the target repository.
- Run architext validate {target_display} before claiming completion.

Required finish:
- Summarize changed data files.
- Summarize covered architecture areas.
- Summarize remaining uncertainty.
- Report validation result.",
        target_display = target.display()
    );
}
