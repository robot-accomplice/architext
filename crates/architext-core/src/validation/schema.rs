//! Schema-validation layer: validates each data file against its JSON Schema
//! (draft 2020-12) using the `jsonschema` crate, mirroring the ajv-based
//! validation in `viewer/tools/validate-architext.mjs`.

use std::path::Path;

use include_dir::{include_dir, Dir};
use serde_json::Value;

/// The JSON schemas, embedded at compile time from the repo's `viewer/schema/`.
/// Lets an installed native binary validate with no `viewer/schema/` on disk;
/// `read_schema_json` prefers an on-disk schema (dev / explicit override) and
/// falls back to this embedded copy.
static EMBEDDED_SCHEMAS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../viewer/schema");

/// Read a schema by filename: prefer the on-disk `schema_dir/<file>` (dev or an
/// explicit override), else the schema embedded in the binary.
fn read_schema_json(schema_dir: &Path, file: &str) -> Result<Value, String> {
    let path = schema_dir.join(file);
    if let Ok(text) = std::fs::read_to_string(&path) {
        return serde_json::from_str(&text)
            .map_err(|e| format!("invalid JSON in {}: {}", path.display(), e));
    }
    match EMBEDDED_SCHEMAS.get_file(file).and_then(|f| f.contents_utf8()) {
        Some(text) => serde_json::from_str(text)
            .map_err(|e| format!("invalid JSON in embedded schema {file}: {e}")),
        None => Err(format!(
            "schema not found: {file} (no {} on disk, and not embedded)",
            path.display()
        )),
    }
}

/// Mapping from logical file key (as used in `manifest.files`) to the
/// corresponding schema filename in the schema directory.
struct FileSchema {
    /// Key in `manifest.files`
    key: &'static str,
    /// Schema filename (relative to schema dir)
    schema_file: &'static str,
    /// Whether this file is required (false = skip if absent from manifest)
    required: bool,
}

/// Files validated against their schemas. Order matches the JS validator.
const FILE_SCHEMAS: &[FileSchema] = &[
    FileSchema { key: "nodes",              schema_file: "nodes.schema.json",              required: true  },
    FileSchema { key: "flows",              schema_file: "flows.schema.json",              required: true  },
    FileSchema { key: "views",              schema_file: "views.schema.json",              required: true  },
    FileSchema { key: "dataClassification", schema_file: "data-classification.schema.json",required: true  },
    FileSchema { key: "decisions",          schema_file: "decisions.schema.json",          required: true  },
    FileSchema { key: "risks",              schema_file: "risks.schema.json",              required: true  },
    FileSchema { key: "glossary",           schema_file: "glossary.schema.json",           required: true  },
    FileSchema { key: "rules",              schema_file: "rules.schema.json",              required: false },
    FileSchema { key: "roadmap",            schema_file: "roadmap.schema.json",            required: false },
    FileSchema { key: "notes",              schema_file: "notes.schema.json",              required: false },
];

/// Read and parse a JSON file, returning an error string on failure.
fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("invalid JSON in {}: {}", path.display(), e))
}

/// Build a draft-2020-12 validator for the given schema value with format
/// validation enabled (mirrors `Ajv2020 + addFormats` in the JS validator).
fn build_validator(schema: &Value) -> Result<jsonschema::Validator, String> {
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(schema)
        .map_err(|e| format!("failed to compile schema: {e}"))
}

/// Validate `instance` against `schema`, appending human-readable error
/// strings to `errors` with `label` as the context prefix.
fn validate_instance(
    label: &str,
    validator: &jsonschema::Validator,
    instance: &Value,
    errors: &mut Vec<String>,
) {
    for err in validator.iter_errors(instance) {
        let path = err.instance_path().to_string();
        if path.is_empty() {
            errors.push(format!("{}: {}", label, err));
        } else {
            errors.push(format!("{}{}: {}", label, path, err));
        }
    }
}

/// Run schema validation for all data files in `data_dir`, using schemas from
/// `schema_dir`. Validates `manifest.json` and every file listed in
/// `manifest.files` that has a corresponding schema entry.
///
/// This mirrors the ajv schema-validation portion of
/// `viewer/tools/validate-architext.mjs main()`.
pub fn validate_schema(data_dir: &Path, schema_dir: &Path, errors: &mut Vec<String>) {
    // --- manifest ------------------------------------------------------------
    let manifest_path = data_dir.join("manifest.json");
    let manifest = match read_json(&manifest_path) {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            return; // cannot continue without manifest
        }
    };

    match read_schema_json(schema_dir, "manifest.schema.json") {
        Ok(schema) => match build_validator(&schema) {
            Ok(validator) => validate_instance("manifest", &validator, &manifest, errors),
            Err(e) => errors.push(e),
        },
        Err(e) => errors.push(e),
    }

    // Extract `manifest.files` object.
    let files = match manifest.get("files").and_then(Value::as_object) {
        Some(f) => f,
        None => {
            errors.push("manifest.files is missing or not an object".to_string());
            return;
        }
    };

    // --- per-file schema validation ------------------------------------------
    for fs in FILE_SCHEMAS {
        let file_path_val = match files.get(fs.key) {
            Some(v) => v,
            None => {
                if fs.required {
                    errors.push(format!("manifest.files.{} is missing", fs.key));
                }
                continue;
            }
        };

        let rel_path = match file_path_val.as_str() {
            Some(s) => s,
            None => {
                errors.push(format!("manifest.files.{} is not a string", fs.key));
                continue;
            }
        };

        let data_path = data_dir.join(rel_path);
        let instance = match read_json(&data_path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };

        let schema = match read_schema_json(schema_dir, fs.schema_file) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };

        let validator = match build_validator(&schema) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };

        validate_instance(fs.key, &validator, &instance, errors);
    }
}
