mod validation;
pub mod domain;
pub mod json_write;
pub mod status;

use std::path::Path;

/// The outcome of validating an Architext data directory.
#[derive(Debug, PartialEq)]
pub struct ValidationOutcome {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// Validate an Architext data directory against the Architext JSON schemas
/// and cross-file reference integrity rules.
///
/// `data_dir`   – directory containing `manifest.json` and the data files it
///               references.
/// `schema_dir` – directory containing the `*.schema.json` files
///               (e.g. `viewer/schema/` in the repo root).
///
/// Returns a `ValidationOutcome` with `ok = errors.is_empty()`.
pub fn validate_data_dir(data_dir: &Path, schema_dir: &Path) -> ValidationOutcome {
    let mut errors = Vec::new();
    validation::schema::validate_schema(data_dir, schema_dir, &mut errors);
    validation::references::validate_references(data_dir, &mut errors);
    validation::code_graph::validate_code_graph(data_dir, &mut errors);

    // Load manifest to check for optional releases/roadmap sections.
    let manifest = read_manifest(data_dir);
    let files = manifest.as_ref().and_then(|m| m.get("files")).and_then(serde_json::Value::as_object);

    // Release checks (if releases present in manifest).
    let releases = files
        .and_then(|f| f.get("releases"))
        .and_then(serde_json::Value::as_str)
        .and_then(|rel| {
            validation::release::validate_release_data(data_dir, rel, &mut errors)
        });

    // Roadmap-target checks (if both roadmap and releases are present).
    if let Some(ref r) = releases {
        if let Some(roadmap_rel) = files.and_then(|f| f.get("roadmap")).and_then(serde_json::Value::as_str) {
            let roadmap_path = data_dir.join(roadmap_rel);
            if let Ok(text) = std::fs::read_to_string(&roadmap_path) {
                if let Ok(roadmap) = serde_json::from_str::<serde_json::Value>(&text) {
                    validation::release::validate_roadmap_release_targets(&roadmap, r, &mut errors);
                }
            }
        }
    }

    ValidationOutcome { ok: errors.is_empty(), errors }
}

/// Read manifest.json from the data directory; returns None on failure
/// (errors already reported by the schema/reference layers).
fn read_manifest(data_dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(data_dir.join("manifest.json")).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn schema_dir() -> PathBuf {
        // Repo root is three levels up from crates/architext-core/src/
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap() // crates/
            .parent().unwrap() // repo root
            .join("viewer")
            .join("schema")
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("conformance")
            .join(name)
    }

    /// RED → GREEN: invalid-schema-missing-field must be rejected.
    /// The nodes.json in this fixture is missing the required `type` field.
    #[test]
    fn invalid_schema_missing_field_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-schema-missing-field"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(!outcome.errors.is_empty());
    }

    /// RED → GREEN: invalid-duplicate-id must be rejected.
    /// The nodes.json contains two nodes with id "node-a".
    #[test]
    fn invalid_duplicate_id_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-duplicate-id"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e.contains("nodes contains duplicate id \"node-a\"")),
            "expected duplicate-id error; got: {:?}",
            outcome.errors
        );
    }

    /// RED → GREEN: invalid-dangling-ref must be rejected with the exact error string.
    /// flow-one step s1.to references "node-does-not-exist".
    #[test]
    fn invalid_dangling_ref_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-dangling-ref"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e == "flow flow-one step s1.to references unknown id \"node-does-not-exist\""),
            "expected dangling-ref error; got: {:?}",
            outcome.errors
        );
    }

    /// RED → GREEN: invalid-release-stale-summary must be rejected.
    /// The index summary text differs from what releaseSummaryFromDetail generates.
    #[test]
    fn invalid_release_stale_summary_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-release-stale-summary"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e.contains("index summary is stale; regenerate Release Truth history")),
            "expected stale-summary error; got: {:?}",
            outcome.errors
        );
    }

    /// RED → GREEN: invalid-release-missing-released-at must be rejected.
    /// The index summary for a completed release is missing releasedAt.
    #[test]
    fn invalid_release_missing_released_at_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-release-missing-released-at"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e.contains("releasedAt is required for completed entries")),
            "expected releasedAt error; got: {:?}",
            outcome.errors
        );
    }

    /// RED → GREEN: invalid-roadmap-bad-target-release must be rejected.
    /// A roadmap item's targetReleaseId references a non-existent release id.
    #[test]
    fn invalid_roadmap_bad_target_release_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-roadmap-bad-target-release"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e.contains("targetReleaseId references unknown id")),
            "expected roadmap-target error; got: {:?}",
            outcome.errors
        );
    }

    /// The real architext self-hosted data must be accepted.
    #[test]
    fn valid_architext_data_is_accepted() {
        let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap()
            .join("docs")
            .join("architext")
            .join("data");
        let outcome = validate_data_dir(&data_dir, &schema_dir());
        assert!(outcome.ok, "expected acceptance; errors: {:?}", outcome.errors);
    }

    #[test]
    fn valid_code_graph_passes() {
        let outcome = validate_data_dir(&fixture("valid-code-graph"), &schema_dir());
        assert!(outcome.ok, "expected pass; errors: {:?}", outcome.errors);
    }

    #[test]
    fn code_graph_digit_leading_id_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-code-graph-bad-id"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
    }

    #[test]
    fn code_graph_dangling_call_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-code-graph-dangling-call"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e|
                e == "code-graph call.to references unknown function id \"m-nonexistent\""),
            "expected dangling-call error; got: {:?}", outcome.errors
        );
    }

    #[test]
    fn code_graph_refusal_is_valid() {
        let outcome = validate_data_dir(&fixture("valid-code-graph-refusal"), &schema_dir());
        assert!(outcome.ok, "refusal must be valid; errors: {:?}", outcome.errors);
    }

    #[test]
    fn code_graph_unknown_major_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-code-graph-bad-major"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e.contains("contract_version") && e.contains("unsupported")),
            "expected version error; got: {:?}", outcome.errors
        );
    }

    #[test]
    fn code_graph_duplicate_function_id_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-code-graph-duplicate-function-id"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e| e == "code-graph functions contains duplicate id \"m-add\""),
            "expected duplicate-function-id error; got: {:?}", outcome.errors
        );
    }

    #[test]
    fn code_graph_dangling_module_function_id_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-code-graph-dangling-module-function-id"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e|
                e == "code-graph module m.function_ids references unknown function id \"m-ghost\""),
            "expected dangling module.function_ids error; got: {:?}", outcome.errors
        );
    }

    #[test]
    fn code_graph_dangling_module_call_is_rejected() {
        let outcome = validate_data_dir(&fixture("invalid-code-graph-dangling-module-call"), &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(
            outcome.errors.iter().any(|e|
                e == "code-graph module_call.to references unknown module id \"m-ghost\""),
            "expected dangling module_call error; got: {:?}", outcome.errors
        );
    }
}
