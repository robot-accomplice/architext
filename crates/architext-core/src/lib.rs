use std::path::Path;

/// The outcome of validating an Architext data directory.
#[derive(Debug, PartialEq)]
pub struct ValidationOutcome {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// Validate an Architext data directory.
///
/// STUB: always accepts. This establishes the RED baseline for the
/// validation-parity conformance harness — valid fixtures MATCH (both accept),
/// invalid fixtures MISMATCH (JS rejects, Rust accepts). Subsequent passes
/// drive to 100 % MATCH by porting the real validation logic.
pub fn validate_data_dir(_dir: &Path) -> ValidationOutcome {
    ValidationOutcome {
        ok: true,
        errors: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_always_accepts() {
        let outcome = validate_data_dir(Path::new("/nonexistent"));
        assert!(outcome.ok);
        assert!(outcome.errors.is_empty());
    }
}
