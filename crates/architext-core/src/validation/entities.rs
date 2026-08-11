//! Referential integrity for the optional persistence model (`entities.json`).
//!
//! Shape is validated by `entities.schema.json` (the schema layer). This module
//! owns the cross-file and internal reference checks a JSON Schema cannot
//! express: that an entity's owner is a real node, its data classes are real
//! classes, and its relationships and foreign keys point at entities that
//! exist.
//!
//! It is a sibling of `code_graph` and `slop_ferret` rather than part of
//! `references`, because `references` is a verbatim mirror of the legacy JS
//! validator (its error strings are reproduced so deep-validate comparisons
//! keep working) and this file has no JS counterpart.
//!
//! Referential integrity is the whole job here. `entities.json` is
//! hand-authored, and every failure mode of a hand-authored file is a reference
//! to something that is not there.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

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

/// Read a JSON file we do not own the errors for. A required core file that is
/// missing or malformed is already reported by the schema and reference layers;
/// reporting it again here would double every such error.
fn read_json_quietly(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Collect the `id` of every object in `items` whose id is a string.
fn id_set(items: &[Value]) -> HashSet<&str> {
    items.iter().filter_map(|i| i.get("id").and_then(Value::as_str)).collect()
}

/// Resolve `manifest.files.<key>` to a document under `data_dir`.
fn load_by_manifest_key(data_dir: &Path, manifest: &Value, key: &str) -> Option<Value> {
    let rel = manifest.get("files")?.get(key)?.as_str()?;
    read_json_quietly(&data_dir.join(rel))
}

/// Push an error if `id` is not in `known`.
fn require_known(
    id: &str,
    known: &HashSet<&str>,
    context: &str,
    kind: &str,
    errors: &mut Vec<String>,
) {
    if !known.contains(id) {
        errors.push(format!("{context} references unknown {kind} id \"{id}\""));
    }
}

/// Validate the entities document referenced by `manifest.files.entities`.
/// No-op (pass) when the key is absent — the file is optional.
pub fn validate_entities(data_dir: &Path, errors: &mut Vec<String>) {
    let manifest = match read_json_quietly(&data_dir.join("manifest.json")) {
        Some(v) => v,
        None => return, // the schema layer owns manifest errors
    };
    let rel = match manifest.get("files").and_then(|f| f.get("entities")).and_then(Value::as_str) {
        Some(r) => r,
        None => return, // optional + unregistered => pass
    };
    let doc = match read_json(&data_dir.join(rel), errors) {
        Some(v) => v,
        None => return,
    };

    let empty: Vec<Value> = Vec::new();
    let entities = doc.get("entities").and_then(Value::as_array).unwrap_or(&empty);

    // --- entity ids must be unique ------------------------------------------
    // Checked before building the id set: a duplicate collapses in the set and
    // would otherwise be invisible.
    let mut seen: HashSet<&str> = HashSet::new();
    for entity in entities {
        if let Some(id) = entity.get("id").and_then(Value::as_str) {
            if !seen.insert(id) {
                errors.push(format!("entities contains duplicate id \"{id}\""));
            }
        }
    }
    let entity_ids = seen;

    // --- cross-file id spaces ------------------------------------------------
    let nodes_doc = load_by_manifest_key(data_dir, &manifest, "nodes");
    let node_ids: HashSet<&str> = nodes_doc
        .as_ref()
        .and_then(|d| d.get("nodes"))
        .and_then(Value::as_array)
        .map(|a| id_set(a))
        .unwrap_or_default();

    let data_doc = load_by_manifest_key(data_dir, &manifest, "dataClassification");
    let data_ids: HashSet<&str> = data_doc
        .as_ref()
        .and_then(|d| d.get("classes"))
        .and_then(Value::as_array)
        .map(|a| id_set(a))
        .unwrap_or_default();

    for entity in entities {
        let eid = match entity.get("id").and_then(Value::as_str) {
            Some(s) => s,
            None => continue, // shape error; the schema layer reports it
        };

        // --- owner ----------------------------------------------------------
        // Only checked when nodes.json actually loaded. Validating against an
        // empty set would turn one unreadable core file into an error on every
        // entity, burying the real cause.
        if let Some(owner) = entity.get("ownerNodeId").and_then(Value::as_str) {
            if nodes_doc.is_some() {
                require_known(
                    owner,
                    &node_ids,
                    &format!("entity {eid}.ownerNodeId"),
                    "node",
                    errors,
                );
            }
        }

        // --- data classes ----------------------------------------------------
        if data_doc.is_some() {
            for class_id in entity
                .get("dataClassIds")
                .and_then(Value::as_array)
                .unwrap_or(&empty)
                .iter()
                .filter_map(Value::as_str)
            {
                require_known(
                    class_id,
                    &data_ids,
                    &format!("entity {eid}.dataClassIds"),
                    "data class",
                    errors,
                );
            }
        }

        // --- relationships: the sole source of rendered edges -----------------
        for rel in entity.get("relationships").and_then(Value::as_array).unwrap_or(&empty) {
            if let Some(to) = rel.get("to").and_then(Value::as_str) {
                require_known(
                    to,
                    &entity_ids,
                    &format!("entity {eid}.relationships.to"),
                    "entity",
                    errors,
                );
            }
        }

        // --- attributes -------------------------------------------------------
        let mut seen_attrs: HashSet<&str> = HashSet::new();
        for attr in entity.get("attributes").and_then(Value::as_array).unwrap_or(&empty) {
            let name = match attr.get("name").and_then(Value::as_str) {
                Some(s) => s,
                None => continue,
            };
            if !seen_attrs.insert(name) {
                errors.push(format!(
                    "entity {eid} contains duplicate attribute name \"{name}\""
                ));
            }
            // A foreign key that RESOLVES but has no matching relationship is
            // valid and deliberately draws no edge -- the viewer annotates the
            // gap in the attribute row. What is checked here is only that the
            // referent exists at all.
            if let Some(target) = attr.get("references").and_then(Value::as_str) {
                require_known(
                    target,
                    &entity_ids,
                    &format!("entity {eid} attribute \"{name}\""),
                    "entity",
                    errors,
                );
            }
        }
    }
}
