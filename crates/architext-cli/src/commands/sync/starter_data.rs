//! Port of `writeStarterData` and `writeStarterReleaseData` from `architext-cli.mjs`.
//!
//! All JSON is serialised via `write_json_string` (byte-identical to JS `writeJson`).

use std::path::Path;

use architext_core::json_write::write_json_string;
use serde_json::json;

use super::target_layout::data_dir;
use super::timestamp::now_iso;

/// Port of JS `slugify(value)`.
fn slugify(value: &str) -> String {
    let slug = value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    // replace runs of dashes, strip leading/trailing
    let mut out = String::new();
    let mut last_dash = true;
    for c in slug.chars() {
        if c == '-' {
            if !last_dash {
                out.push('-');
            }
            last_dash = true;
        } else {
            out.push(c);
            last_dash = false;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    let out = if out.is_empty() { "target-project".to_string() } else { out };
    // slice(0,64)
    let out: String = out.chars().take(64).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "target-project".to_string() } else { out }
}

fn write_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, write_json_string(value).as_bytes())
}

/// Port of `writeStarterReleaseData(targetDataDir)`.
pub fn write_starter_release_data(target_data_dir: &Path) -> std::io::Result<()> {
    let release_dir = target_data_dir.join("releases");
    std::fs::create_dir_all(&release_dir)?;

    let release_id = "initial-architext-buildout";
    let last_updated = now_iso();

    // index.json
    write_json(
        &release_dir.join("index.json"),
        &json!({
            "currentReleaseId": release_id,
            "releases": [
                {
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
                }
            ]
        }),
    )?;

    // <release-id>.json
    write_json(
        &release_dir.join(format!("{release_id}.json")),
        &json!({
            "id": release_id,
            "version": "0.1.0",
            "name": "Initial Architext build-out",
            "status": "planned",
            "posture": "at-risk",
            "summary": "Replace starter architecture and release data with project-specific facts.",
            "targetWindow": "Before claiming architecture documentation is current",
            "lastUpdated": last_updated,
            "updateSource": "architext sync starter data",
            "scope": {
                "required": [
                    {
                        "id": "replace-starter-architecture-data",
                        "title": "Replace starter architecture data",
                        "kind": "documentation",
                        "status": "planned",
                        "summary": "Inspect the repository and replace starter Architext JSON with source-backed project facts.",
                        "owner": "Project maintainers",
                        "workstreamId": "architecture-buildout",
                        "dependsOn": [],
                        "evidence": ["architext validate"]
                    }
                ],
                "planned": [],
                "stretch": [],
                "deferred": [],
                "outOfScope": []
            },
            "workstreams": [
                {
                    "id": "architecture-buildout",
                    "name": "Architecture build-out",
                    "owner": "Project maintainers",
                    "status": "planned",
                    "posture": "at-risk",
                    "summary": "Replace starter architecture facts, release facts, and validation evidence before relying on Architext.",
                    "progress": 0,
                    "itemIds": ["replace-starter-architecture-data"],
                    "evidence": ["architext validate"]
                }
            ],
            "blockers": [
                {
                    "id": "starter-data-not-replaced",
                    "title": "Starter data is not project truth",
                    "severity": "high",
                    "status": "blocked",
                    "owner": "Project maintainers",
                    "summary": "The project has validating starter data, but it has not yet been replaced with source-backed architecture and release facts.",
                    "nextAction": "Run the agent-assisted Architext build-out workflow and review the JSON diff.",
                    "itemIds": ["replace-starter-architecture-data"],
                    "evidenceNeeded": ["Source-backed JSON updates", "architext validate"]
                }
            ],
            "milestones": [
                {
                    "id": "starter-replaced",
                    "label": "Starter data replaced",
                    "status": "planned",
                    "targetWindow": "Initial documentation pass",
                    "order": 1,
                    "itemIds": ["replace-starter-architecture-data"]
                }
            ],
            "dependencies": [],
            "evidence": [
                {
                    "id": "starter-validation",
                    "label": "architext validate",
                    "kind": "test",
                    "status": "planned"
                }
            ]
        }),
    )?;

    Ok(())
}

/// Port of `writeStarterData(target, version)`.
pub fn write_starter_data(target: &Path) -> std::io::Result<()> {
    let target_data_dir = data_dir(target);
    std::fs::create_dir_all(&target_data_dir)?;

    let project_name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());
    let project_id = slugify(&project_name);
    let system_id = format!("{project_id}-system");
    let container_id = format!("{project_id}-container");
    let component_id = format!("{project_id}-component");
    let actor_id = "project-team";
    let data_id = "architecture-knowledge";
    let flow_id = "architecture-buildout";

    let generated_at = now_iso();

    // manifest.json
    write_json(
        &target_data_dir.join("manifest.json"),
        &json!({
            "schemaVersion": "1.5.0",
            "project": {
                "id": project_id,
                "name": project_name,
                "summary": "Architext has been installed. Replace this starter model with the real project architecture."
            },
            "generatedAt": generated_at,
            "defaultViewId": "system-map",
            "files": {
                "nodes": "nodes.json",
                "flows": "flows.json",
                "views": "views.json",
                "dataClassification": "data-classification.json",
                "decisions": "decisions.json",
                "risks": "risks.json",
                "glossary": "glossary.json",
                "rules": "rules.json",
                "releases": "releases/index.json"
            },
            "notes": [
                "Starter data only. Ask an agent to inspect the codebase and build out docs/architext/data/**/*.json.",
                "Do not treat this starter model as architecture documentation for the target project."
            ]
        }),
    )?;

    // nodes.json
    write_json(
        &target_data_dir.join("nodes.json"),
        &json!({
            "nodes": [
                {
                    "id": actor_id,
                    "type": "actor",
                    "name": "Project Team",
                    "summary": "Placeholder actor for the team or user initiating the Architext build-out.",
                    "responsibilities": ["Replace starter data with real architecture facts"],
                    "owner": "Project maintainers",
                    "sourcePaths": [],
                    "runtime": "Repository workflow",
                    "interfaces": ["Architext JSON"],
                    "dependencies": [&system_id],
                    "dataHandled": [data_id],
                    "security": ["Unknown until architecture build-out is complete"],
                    "observability": ["Unknown until architecture build-out is complete"],
                    "relatedFlows": [flow_id],
                    "relatedDecisions": [],
                    "knownRisks": ["architext-starter-data"],
                    "verification": ["architext validate"]
                },
                {
                    "id": &system_id,
                    "type": "software-system",
                    "name": &project_name,
                    "summary": "Placeholder system boundary. Replace with the real project systems, services, stores, flows, and dependencies.",
                    "responsibilities": ["Pending architecture discovery"],
                    "owner": "Project maintainers",
                    "sourcePaths": [],
                    "runtime": "Unknown until architecture build-out is complete",
                    "interfaces": ["Unknown until architecture build-out is complete"],
                    "dependencies": [],
                    "dataHandled": [data_id],
                    "security": ["Unknown until architecture build-out is complete"],
                    "observability": ["Unknown until architecture build-out is complete"],
                    "relatedFlows": [flow_id],
                    "relatedDecisions": ["architext-buildout-required"],
                    "knownRisks": ["architext-starter-data"],
                    "verification": ["architext validate"]
                },
                {
                    "id": &container_id,
                    "type": "service",
                    "name": format!("{project_name} service placeholder"),
                    "summary": "Placeholder container inside the system boundary. Replace with real deployable units during architecture build-out.",
                    "responsibilities": ["Pending container discovery"],
                    "owner": "Project maintainers",
                    "sourcePaths": [],
                    "runtime": "Unknown until architecture build-out is complete",
                    "interfaces": ["Unknown until architecture build-out is complete"],
                    "dependencies": [],
                    "dataHandled": [data_id],
                    "security": ["Unknown until architecture build-out is complete"],
                    "observability": ["Unknown until architecture build-out is complete"],
                    "relatedFlows": [flow_id],
                    "relatedDecisions": ["architext-buildout-required"],
                    "knownRisks": ["architext-starter-data"],
                    "verification": ["architext validate"]
                },
                {
                    "id": &component_id,
                    "type": "module",
                    "name": format!("{project_name} component placeholder"),
                    "summary": "Placeholder component. Replace with real components inside a selected container during architecture build-out.",
                    "responsibilities": ["Pending component discovery"],
                    "owner": "Project maintainers",
                    "sourcePaths": [],
                    "runtime": "Unknown until architecture build-out is complete",
                    "interfaces": ["Unknown until architecture build-out is complete"],
                    "dependencies": [],
                    "dataHandled": [data_id],
                    "security": ["Unknown until architecture build-out is complete"],
                    "observability": ["Unknown until architecture build-out is complete"],
                    "relatedFlows": [flow_id],
                    "relatedDecisions": ["architext-buildout-required"],
                    "knownRisks": ["architext-starter-data"],
                    "verification": ["architext validate"]
                }
            ]
        }),
    )?;

    // flows.json
    write_json(
        &target_data_dir.join("flows.json"),
        &json!({
            "flows": [
                {
                    "id": flow_id,
                    "name": "Architext build-out required",
                    "status": "planned",
                    "summary": "Starter flow showing that architecture data still needs to be generated from the target repository.",
                    "trigger": "Architext installed into the project",
                    "actors": [actor_id],
                    "steps": [
                        {
                            "id": "inspect-project",
                            "from": actor_id,
                            "to": &system_id,
                            "action": "inspectCodebaseAndReplaceStarterData",
                            "summary": "An agent should inspect the repository and replace every starter JSON file with real architecture data.",
                            "data": [data_id]
                        }
                    ],
                    "guarantees": ["Validation passes for starter data"],
                    "failureBehavior": ["Rendered site is not useful until project-specific data replaces the starter model"],
                    "observability": ["Validation output"],
                    "verification": ["architext validate"],
                    "knownGaps": ["All project architecture facts are pending discovery"]
                }
            ]
        }),
    )?;

    // views.json
    write_json(
        &target_data_dir.join("views.json"),
        &json!({
            "views": [
                {
                    "id": "system-map",
                    "name": "System Map",
                    "type": "system-map",
                    "summary": "Starter view. Replace with the real project system map.",
                    "lanes": [
                        { "id": "people", "name": "People", "nodeIds": [actor_id] },
                        { "id": "system", "name": "System", "nodeIds": [&system_id] }
                    ]
                },
                {
                    "id": "dataflow",
                    "name": "Dataflow",
                    "type": "dataflow",
                    "summary": "Starter dataflow. Replace with real data movement.",
                    "lanes": [
                        { "id": "source", "name": "Source", "nodeIds": [actor_id] },
                        { "id": "target", "name": "Target", "nodeIds": [&system_id] }
                    ]
                },
                {
                    "id": "sequence",
                    "name": "Sequence",
                    "type": "sequence",
                    "summary": "Starter sequence for the build-out flow.",
                    "lanes": [
                        { "id": "participants", "name": "Participants", "nodeIds": [actor_id, &system_id] }
                    ]
                },
                {
                    "id": "deployment",
                    "name": "Deployment",
                    "type": "deployment",
                    "summary": "Starter deployment view. Replace with real runtime placement.",
                    "lanes": [
                        { "id": "unknown", "name": "Unknown", "nodeIds": [&system_id] }
                    ]
                },
                {
                    "id": "c4-context",
                    "name": "C4 Context",
                    "type": "c4-context",
                    "summary": "Starter C4 context. Replace with real actors, system boundary, and external systems.",
                    "lanes": [
                        { "id": "people", "name": "People", "nodeIds": [actor_id] },
                        { "id": "system", "name": "System", "nodeIds": [&system_id] }
                    ]
                },
                {
                    "id": "c4-container",
                    "name": "C4 Container",
                    "type": "c4-container",
                    "summary": "Starter C4 container view. Replace with deployable units and dependencies.",
                    "scopeNodeId": &system_id,
                    "lanes": [
                        { "id": "containers", "name": "Containers", "nodeIds": [&container_id] }
                    ]
                },
                {
                    "id": "c4-component",
                    "name": "C4 Component",
                    "type": "c4-component",
                    "summary": "Starter C4 component view. Replace with components inside a selected container.",
                    "scopeNodeId": &container_id,
                    "lanes": [
                        { "id": "components", "name": "Components", "nodeIds": [&component_id] }
                    ]
                }
            ]
        }),
    )?;

    // data-classification.json
    write_json(
        &target_data_dir.join("data-classification.json"),
        &json!({
            "classes": [
                {
                    "id": data_id,
                    "name": "Architecture Knowledge",
                    "sensitivity": "medium",
                    "handling": "Review generated architecture facts before treating them as project documentation."
                }
            ]
        }),
    )?;

    // decisions.json
    write_json(
        &target_data_dir.join("decisions.json"),
        &json!({
            "decisions": [
                {
                    "id": "architext-buildout-required",
                    "status": "planned",
                    "title": "Replace starter Architext data",
                    "context": "Architext was installed with neutral starter data.",
                    "decision": "An agent must inspect the target repository and replace docs/architext/data/*.json with project-specific architecture facts.",
                    "consequences": [
                        "The site validates immediately",
                        "The starter model is intentionally not useful as final documentation"
                    ],
                    "relatedNodes": [&system_id],
                    "relatedFlows": [flow_id]
                }
            ]
        }),
    )?;

    // risks.json
    write_json(
        &target_data_dir.join("risks.json"),
        &json!({
            "risks": [
                {
                    "id": "architext-starter-data",
                    "title": "Starter data is not project architecture",
                    "category": "technical",
                    "severity": "high",
                    "status": "open",
                    "summary": "The installed Architext data is a placeholder until an agent builds out the real architecture model.",
                    "mitigations": [
                        "Run the agent-assisted JSON build-out workflow",
                        "Review generated JSON diffs",
                        "Run architext validate"
                    ],
                    "relatedNodes": [&system_id],
                    "relatedFlows": [flow_id]
                }
            ]
        }),
    )?;

    // glossary.json
    write_json(
        &target_data_dir.join("glossary.json"),
        &json!({
            "terms": [
                {
                    "term": "Architext starter data",
                    "definition": "A neutral validating placeholder installed into new projects before real architecture data is generated."
                }
            ]
        }),
    )?;

    // rules.json
    write_json(
        &target_data_dir.join("rules.json"),
        &json!({
            "rules": [
                {
                    "id": "replace-starter-data",
                    "title": "Replace starter data",
                    "summary": "Replace neutral starter data with source-backed architecture, release, and project rules before treating Architext as current.",
                    "category": "project",
                    "criticality": "critical",
                    "order": 10,
                    "source": "maintainer",
                    "rationale": "Fresh installs validate immediately, but starter data is not project-specific documentation.",
                    "appliesTo": ["initial build-out", "agent maintenance", "validation"],
                    "protection": { "edit": true, "delete": true }
                }
            ]
        }),
    )?;

    // releases/
    write_starter_release_data(&target_data_dir)?;

    Ok(())
}
