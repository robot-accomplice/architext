//! Reference-integrity validation layer.
//!
//! Mirrors the `requireUnique` and `validateReferences` functions in
//! `viewer/tools/validate-architext.mjs` (the inline ones called by `main()`).
//! Error strings are reproduced verbatim so the product's deep-validate
//! comparisons continue to work.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Push an error if `id` is not in `known`.
/// Mirrors: `function requireKnown(id, known, context, errors)`
fn require_known(id: &str, known: &HashSet<&str>, context: &str, errors: &mut Vec<String>) {
    if !known.contains(id) {
        errors.push(format!("{context} references unknown id \"{id}\""));
    }
}

/// Push an error for every duplicate `id` field among `items`.
/// Mirrors: `function requireUnique(items, label, errors)`
fn require_unique<'a>(
    items: impl Iterator<Item = &'a Value>,
    label: &str,
    errors: &mut Vec<String>,
) {
    let mut seen: HashSet<&str> = HashSet::new();
    for item in items {
        let id = match item.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };
        if seen.contains(id) {
            errors.push(format!("{label} contains duplicate id \"{id}\""));
        }
        seen.insert(id);
    }
}

/// Collect all `id` string values from an array of objects into a `HashSet`.
fn id_set(items: &[Value]) -> HashSet<&str> {
    items
        .iter()
        .filter_map(|v| v.get("id").and_then(Value::as_str))
        .collect()
}

/// Read a JSON file; return `None` and push an error on failure.
fn read_json(path: &Path, errors: &mut Vec<String>) -> Option<Value> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("cannot read {}: {}", path.display(), e));
            return None;
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(format!("invalid JSON in {}: {}", path.display(), e));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run duplicate-id and cross-file reference checks on the data directory.
///
/// Mirrors `requireUnique(…)` calls and `validateReferences(model, errors)`
/// from `viewer/tools/validate-architext.mjs main()`.
///
/// Called unconditionally after schema validation (JS does the same —
/// `validateReferences` is called even when schema errors were pushed).
pub fn validate_references(data_dir: &Path, errors: &mut Vec<String>) {
    // --- load manifest -------------------------------------------------------
    let manifest_path = data_dir.join("manifest.json");
    let manifest = match read_json(&manifest_path, errors) {
        Some(v) => v,
        None => return, // cannot continue
    };

    let files = match manifest.get("files").and_then(Value::as_object) {
        Some(f) => f,
        None => return, // schema layer already errors on this
    };

    // Helper: load a required file by manifest key.
    macro_rules! load_required {
        ($key:expr) => {{
            let rel = match files.get($key).and_then(Value::as_str) {
                Some(s) => s,
                None => return, // schema layer already reported this
            };
            match read_json(&data_dir.join(rel), errors) {
                Some(v) => v,
                None => return,
            }
        }};
    }

    // Helper: load an optional file by manifest key (None if absent).
    macro_rules! load_optional {
        ($key:expr) => {{
            match files.get($key).and_then(Value::as_str) {
                Some(rel) => read_json(&data_dir.join(rel), errors),
                None => None,
            }
        }};
    }

    let nodes_doc = load_required!("nodes");
    let flows_doc = load_required!("flows");
    let views_doc = load_required!("views");
    let data_doc = load_required!("dataClassification");
    let decisions_doc = load_required!("decisions");
    let risks_doc = load_required!("risks");
    let notes_doc = load_optional!("notes");
    let roadmap_doc = load_optional!("roadmap");
    let rules_doc = load_optional!("rules");

    // --- extract arrays ------------------------------------------------------
    let empty = vec![];

    let nodes = nodes_doc.get("nodes").and_then(Value::as_array).unwrap_or(&empty);
    let flows = flows_doc.get("flows").and_then(Value::as_array).unwrap_or(&empty);
    let views = views_doc.get("views").and_then(Value::as_array).unwrap_or(&empty);
    let data_classes = data_doc.get("classes").and_then(Value::as_array).unwrap_or(&empty);
    let decisions = decisions_doc.get("decisions").and_then(Value::as_array).unwrap_or(&empty);
    let risks = risks_doc.get("risks").and_then(Value::as_array).unwrap_or(&empty);

    // --- requireUnique (mirrors JS main() call order) ------------------------
    require_unique(nodes.iter(), "nodes", errors);
    require_unique(flows.iter(), "flows", errors);
    require_unique(views.iter(), "views", errors);
    require_unique(data_classes.iter(), "dataClassification.classes", errors);
    require_unique(decisions.iter(), "decisions", errors);
    require_unique(risks.iter(), "risks", errors);
    if let Some(ref rm) = roadmap_doc {
        let items = rm.get("items").and_then(Value::as_array).unwrap_or(&empty);
        require_unique(items.iter(), "roadmap.items", errors);
    }
    if let Some(ref rl) = rules_doc {
        let rules_arr = rl.get("rules").and_then(Value::as_array).unwrap_or(&empty);
        require_unique(rules_arr.iter(), "rules", errors);
    }
    if let Some(ref nd) = notes_doc {
        let notes_arr = nd.get("notes").and_then(Value::as_array).unwrap_or(&empty);
        require_unique(notes_arr.iter(), "notes", errors);
    }

    // --- build id sets -------------------------------------------------------
    let node_ids: HashSet<&str> = id_set(nodes);
    let flow_ids: HashSet<&str> = id_set(flows);
    let data_ids: HashSet<&str> = id_set(data_classes);
    let decision_ids: HashSet<&str> = id_set(decisions);
    let risk_ids: HashSet<&str> = id_set(risks);
    let view_ids: HashSet<&str> = id_set(views);

    // --- manifest.defaultViewId ----------------------------------------------
    if let Some(default_view_id) = manifest.get("defaultViewId").and_then(Value::as_str) {
        require_known(default_view_id, &view_ids, "manifest.defaultViewId", errors);
    }

    // --- nodes ---------------------------------------------------------------
    for node in nodes {
        let node_id = match node.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };
        for id in arr_strs(node, "dependencies") {
            require_known(id, &node_ids, &format!("node {node_id}.dependencies"), errors);
        }
        for id in arr_strs(node, "dataHandled") {
            require_known(id, &data_ids, &format!("node {node_id}.dataHandled"), errors);
        }
        for id in arr_strs(node, "relatedFlows") {
            require_known(id, &flow_ids, &format!("node {node_id}.relatedFlows"), errors);
        }
        for id in arr_strs(node, "relatedDecisions") {
            require_known(id, &decision_ids, &format!("node {node_id}.relatedDecisions"), errors);
        }
        for id in arr_strs(node, "knownRisks") {
            require_known(id, &risk_ids, &format!("node {node_id}.knownRisks"), errors);
        }
    }

    // --- flows ---------------------------------------------------------------
    for flow in flows {
        let flow_id = match flow.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };
        for id in arr_strs(flow, "actors") {
            require_known(id, &node_ids, &format!("flow {flow_id}.actors"), errors);
        }

        let steps = flow.get("steps").and_then(Value::as_array).unwrap_or(&empty);

        // Build step id set (with duplicate detection) — mirrors JS two-pass.
        let mut step_ids: HashSet<&str> = HashSet::new();
        for step in steps.iter() {
            let step_id = match step.get("id").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            if step_ids.contains(step_id) {
                errors.push(format!(
                    "flow {flow_id} contains duplicate step id \"{step_id}\""
                ));
            }
            step_ids.insert(step_id);
        }

        // Validate step references.
        for step in steps.iter() {
            let step_id = match step.get("id").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            if let Some(from) = step.get("from").and_then(Value::as_str) {
                require_known(from, &node_ids, &format!("flow {flow_id} step {step_id}.from"), errors);
            }
            if let Some(to) = step.get("to").and_then(Value::as_str) {
                require_known(to, &node_ids, &format!("flow {flow_id} step {step_id}.to"), errors);
            }
            if let Some(return_of) = step.get("returnOf").and_then(Value::as_str) {
                require_known(return_of, &step_ids, &format!("flow {flow_id} step {step_id}.returnOf"), errors);
            }
            for id in arr_strs(step, "data") {
                require_known(id, &data_ids, &format!("flow {flow_id} step {step_id}.data"), errors);
            }
        }

        // sequenceFrames
        let frames = flow.get("sequenceFrames").and_then(Value::as_array);
        if let Some(frames) = frames {
            for frame in frames {
                let frame_id = match frame.get("id").and_then(Value::as_str) {
                    Some(s) => s,
                    None => continue,
                };
                for id in arr_strs(frame, "stepIds") {
                    require_known(
                        id,
                        &step_ids,
                        &format!("flow {flow_id} sequenceFrame {frame_id}.stepIds"),
                        errors,
                    );
                }
            }
        }
    }

    // --- views ---------------------------------------------------------------
    for view in views {
        let view_id = match view.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };
        let lanes = view.get("lanes").and_then(Value::as_array).unwrap_or(&empty);
        let mut lane_ids: HashSet<&str> = HashSet::new();
        for lane in lanes {
            let lane_id = match lane.get("id").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            if lane_ids.contains(lane_id) {
                errors.push(format!(
                    "view {view_id} contains duplicate lane id \"{lane_id}\""
                ));
            }
            lane_ids.insert(lane_id);
            for id in arr_strs(lane, "nodeIds") {
                require_known(id, &node_ids, &format!("view {view_id} lane {lane_id}"), errors);
            }
        }
    }

    // --- decisions -----------------------------------------------------------
    for decision in decisions {
        let decision_id = match decision.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };
        for id in arr_strs(decision, "relatedNodes") {
            require_known(id, &node_ids, &format!("decision {decision_id}.relatedNodes"), errors);
        }
        for id in arr_strs(decision, "relatedFlows") {
            require_known(id, &flow_ids, &format!("decision {decision_id}.relatedFlows"), errors);
        }
    }

    // --- risks ---------------------------------------------------------------
    for risk in risks {
        let risk_id = match risk.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue,
        };
        for id in arr_strs(risk, "relatedNodes") {
            require_known(id, &node_ids, &format!("risk {risk_id}.relatedNodes"), errors);
        }
        for id in arr_strs(risk, "relatedFlows") {
            require_known(id, &flow_ids, &format!("risk {risk_id}.relatedFlows"), errors);
        }
    }

    // --- notes ---------------------------------------------------------------
    if let Some(ref nd) = notes_doc {
        let notes_arr = nd.get("notes").and_then(Value::as_array).unwrap_or(&empty);
        // Build the kind → id-set map.
        let ids_by_kind: HashMap<&str, &HashSet<&str>> = [
            ("node", &node_ids),
            ("flow", &flow_ids),
            ("decision", &decision_ids),
            ("risk", &risk_ids),
            ("view", &view_ids),
            ("data-class", &data_ids),
        ]
        .into_iter()
        .collect();

        for note in notes_arr {
            let note_id = match note.get("id").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            let target = match note.get("target") {
                Some(t) => t,
                None => continue,
            };
            let kind = match target.get("kind").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            let target_id = match target.get("id").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            if let Some(known) = ids_by_kind.get(kind) {
                require_known(
                    target_id,
                    known,
                    &format!("note {note_id}.target ({kind})"),
                    errors,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internal utility
// ---------------------------------------------------------------------------

/// Iterate over string values in a JSON array field of an object.
fn arr_strs<'a>(obj: &'a Value, field: &str) -> impl Iterator<Item = &'a str> {
    obj.get(field)
        .and_then(Value::as_array)
        .map(|arr| arr.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| v.as_str())
}
