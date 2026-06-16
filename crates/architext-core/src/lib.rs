mod validation;

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
    ValidationOutcome { ok: errors.is_empty(), errors }
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
}
