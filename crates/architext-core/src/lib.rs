mod validation;

use std::path::Path;

/// The outcome of validating an Architext data directory.
#[derive(Debug, PartialEq)]
pub struct ValidationOutcome {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// Validate an Architext data directory against the Architext JSON schemas.
///
/// `data_dir`   – directory containing `manifest.json` and the data files it
///               references.
/// `schema_dir` – directory containing the `*.schema.json` files
///               (e.g. `viewer/schema/` in the repo root).
///
/// Returns a `ValidationOutcome` with `ok = errors.is_empty()`.
/// Only the schema-validation layer is ported in this pass; reference and
/// release checks (duplicate-id, dangling-ref, release, roadmap-target) are
/// later passes.
pub fn validate_data_dir(data_dir: &Path, schema_dir: &Path) -> ValidationOutcome {
    let mut errors = Vec::new();
    validation::schema::validate_schema(data_dir, schema_dir, &mut errors);
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

    /// RED → GREEN: invalid-schema-missing-field must be rejected.
    /// The nodes.json in this fixture is missing the required `type` field.
    #[test]
    fn invalid_schema_missing_field_is_rejected() {
        let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("conformance")
            .join("invalid-schema-missing-field");
        let outcome = validate_data_dir(&data_dir, &schema_dir());
        assert!(!outcome.ok, "expected rejection; errors: {:?}", outcome.errors);
        assert!(!outcome.errors.is_empty());
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
