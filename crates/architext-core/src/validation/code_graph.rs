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
/// Fields a `limitation.evidenced_by` may name.
///
/// Mirrors `disclosure`'s properties in `code-graph.schema.json`. Kept as an
/// explicit list because `evidenced_by` is a POINTER: an id that resolves to
/// nothing is a dangling reference, and the viewer would silently show a
/// limitation whose supporting number never appears. Unlike `scope`/`effect`
/// — producer VOCABULARY, deliberately unpinned so an unknown value degrades —
/// this names OUR own structure, so a mismatch is a real integrity error.
const DISCLOSURE_FIELDS: &[&str] =
    &["nodes", "roots", "generated", "dynamic_edges", "root_ratio"];

/// Referential integrity for the disclosure surface.
pub fn validate_limitations(doc: &Value, errors: &mut Vec<String>) {
    let limitations = match doc.get("limitations").and_then(Value::as_array) {
        Some(l) => l,
        None => return, // optional; absent means "not disclosed"
    };
    require_unique(limitations, "code-graph limitations", errors);

    let has_disclosure = doc.get("disclosure").and_then(Value::as_object).is_some();
    for lim in limitations {
        let Some(ev) = lim.get("evidenced_by").and_then(Value::as_str) else { continue };
        let id = lim.get("id").and_then(Value::as_str).unwrap_or("?");
        if !DISCLOSURE_FIELDS.contains(&ev) {
            errors.push(format!(
                "code-graph limitation \"{id}\" is evidenced_by \"{ev}\", which is not a disclosure field"
            ));
        } else if !has_disclosure {
            errors.push(format!(
                "code-graph limitation \"{id}\" is evidenced_by \"{ev}\" but the document has no disclosure object"
            ));
        }
    }
}

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

    validate_limitations(&doc, errors);

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

#[cfg(test)]
mod limitation_tests {
    use super::validate_limitations;
    use serde_json::json;

    #[test]
    fn a_dangling_evidence_pointer_is_an_error() {
        // WHY: `evidenced_by` is a POINTER into our own disclosure struct. If it
        // resolves to nothing the viewer shows a limitation whose supporting
        // number never appears — a claim with invisible evidence, which is
        // exactly the shape this field exists to prevent.
        let doc = json!({
            "limitations": [{
                "id": "x", "scope": "backend", "attribution": "magma",
                "description": "d", "effect": "may-omit-edges",
                "evidenced_by": "not_a_field"
            }],
            "disclosure": { "nodes": 1 }
        });
        let mut errors = Vec::new();
        validate_limitations(&doc, &mut errors);
        assert!(errors.iter().any(|e| e.contains("not a disclosure field")), "{errors:?}");
    }

    #[test]
    fn evidence_without_a_disclosure_object_is_an_error() {
        let doc = json!({
            "limitations": [{
                "id": "x", "scope": "backend", "attribution": "magma",
                "description": "d", "effect": "over-approximates-live",
                "evidenced_by": "root_ratio"
            }]
        });
        let mut errors = Vec::new();
        validate_limitations(&doc, &mut errors);
        assert!(errors.iter().any(|e| e.contains("no disclosure object")), "{errors:?}");
    }

    #[test]
    fn unknown_scope_and_effect_are_accepted() {
        // The governance rule, pinned as a test: `scope`/`effect` are producer
        // VOCABULARY and deliberately unpinned, so a value this build predates
        // must pass. Pinning them would reproduce the kind:"init" outage, where
        // one unknown value invalidated a 15MB artifact.
        let doc = json!({
            "limitations": [{
                "id": "future", "scope": "sandbox-mode", "attribution": "magma 9.9",
                "description": "d", "effect": "may-invent-nodes"
            }]
        });
        let mut errors = Vec::new();
        validate_limitations(&doc, &mut errors);
        assert!(errors.is_empty(), "unknown vocabulary must not be an error: {errors:?}");
    }

    #[test]
    fn duplicate_limitation_ids_are_rejected() {
        // ids are the suppression handle a consumer uses; two limitations
        // sharing one makes targeted suppression ambiguous.
        let l = json!({
            "id": "dupe", "scope": "backend", "attribution": "m",
            "description": "d", "effect": "may-omit-edges"
        });
        let doc = json!({ "limitations": [l, l] });
        let mut errors = Vec::new();
        validate_limitations(&doc, &mut errors);
        assert!(errors.iter().any(|e| e.contains("duplicate id")), "{errors:?}");
    }

    #[test]
    fn an_absent_limitations_array_is_not_an_error() {
        // Optional by design: artifacts predating the field stay valid.
        let mut errors = Vec::new();
        validate_limitations(&json!({ "functions": [] }), &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
    }
}
