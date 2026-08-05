//! Code-graph layer semantics for the `magma-code-graph/1` contract.
//!
//! Shape is validated by `code-graph.schema.json` (the schema layer). This
//! module owns contract-version gating, refusal acceptance, and internal
//! referential integrity — checks the generic schema/reference layers do not
//! cover because the code-graph ids live in their own id-space.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

/// Contract id Architext consumes. This string encodes MAJOR only — minor/patch
/// contract revisions do not change it — so an exact match against this
/// constant IS the major gate, and it accepts higher-minor documents by
/// construction. An unknown major is rejected loudly below.
const CONTRACT_ID: &str = "magma-code-graph/1";

/// Read a JSON file, pushing an error on failure. Returns None on failure.
fn read_json(path: &Path, errors: &mut Vec<String>) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(format!("invalid JSON in {}: {}", path.display(), e));
            None
        }
    }
}

/// Push an error for every duplicate `id` among `items`.
fn require_unique(items: &[Value], label: &str, errors: &mut Vec<String>) {
    let mut seen: HashSet<&str> = HashSet::new();
    for item in items {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            if !seen.insert(id) {
                errors.push(format!("{label} contains duplicate id \"{id}\""));
            }
        }
    }
}

/// Validate the code-graph document referenced by `manifest.files.codeGraph`.
/// No-op (pass) when the key is absent — the file is optional.
pub fn validate_code_graph(data_dir: &Path, errors: &mut Vec<String>) {
    let manifest = match read_json(&data_dir.join("manifest.json"), errors) {
        Some(v) => v,
        None => return,
    };
    let rel = match manifest
        .get("files")
        .and_then(|f| f.get("codeGraph"))
        .and_then(Value::as_str)
    {
        Some(r) => r,
        None => return, // optional + unregistered => pass
    };
    let doc = match read_json(&data_dir.join(rel), errors) {
        Some(v) => v,
        None => return,
    };

    // --- contract-version gating (single source of the version error) --------
    let cv = doc.get("contract_version").and_then(Value::as_str).unwrap_or("");
    if cv != CONTRACT_ID {
        errors.push(format!(
            "code-graph contract_version \"{cv}\" is unsupported; this build consumes \"{CONTRACT_ID}\""
        ));
        return;
    }

    // --- refusal: computable=false => valid, nothing further to check --------
    if doc.get("computable").and_then(Value::as_bool) == Some(false) {
        return;
    }

    // --- internal referential integrity -------------------------------------
    let empty: Vec<Value> = Vec::new();
    let functions = doc.get("functions").and_then(Value::as_array).unwrap_or(&empty);
    let modules = doc.get("modules").and_then(Value::as_array).unwrap_or(&empty);
    let calls = doc.get("calls").and_then(Value::as_array).unwrap_or(&empty);
    let module_calls = doc.get("module_calls").and_then(Value::as_array).unwrap_or(&empty);

    require_unique(functions, "code-graph functions", errors);
    require_unique(modules, "code-graph modules", errors);

    let function_ids: HashSet<&str> =
        functions.iter().filter_map(|f| f.get("id").and_then(Value::as_str)).collect();
    let module_ids: HashSet<&str> =
        modules.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).collect();

    for call in calls {
        for field in ["from", "to"] {
            if let Some(id) = call.get(field).and_then(Value::as_str) {
                if !function_ids.contains(id) {
                    errors.push(format!(
                        "code-graph call.{field} references unknown function id \"{id}\""
                    ));
                }
            }
        }
    }

    for m in modules {
        let mid = m.get("id").and_then(Value::as_str).unwrap_or("");
        if let Some(fids) = m.get("function_ids").and_then(Value::as_array) {
            for fid in fids.iter().filter_map(Value::as_str) {
                if !function_ids.contains(fid) {
                    errors.push(format!(
                        "code-graph module {mid}.function_ids references unknown function id \"{fid}\""
                    ));
                }
            }
        }
    }

    for mc in module_calls {
        for field in ["from", "to"] {
            if let Some(id) = mc.get(field).and_then(Value::as_str) {
                if !module_ids.contains(id) {
                    errors.push(format!(
                        "code-graph module_call.{field} references unknown module id \"{id}\""
                    ));
                }
            }
        }
    }
}
