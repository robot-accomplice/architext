//! Slop-ferret sweep semantics.
//!
//! Shape is validated by `slop-ferret.schema.json` (the schema layer). This
//! module owns the two things the generic schema layer cannot express: schema
//! VERSION gating, and reconciling the sweep's own counts against the findings
//! it actually carries.
//!
//! Both exist for the same reason. A slop-ferret sweep is a QA artifact whose
//! entire value is its DENOMINATORS — "N of M checked, K waived, these families
//! not run". A viewer that renders a rate whose denominator disagrees with the
//! data behind it is worse than one that renders nothing: it presents a
//! confidently wrong coverage figure as an authoritative one. So a document
//! whose counts contradict its own findings is rejected here rather than
//! rendered.

use std::path::Path;

use serde_json::Value;

/// Sweep schema version this build understands.
///
/// Gated for the same reason `code_graph`'s `contract_version` is: slop-ferret
/// is a separately-versioned upstream tool, and a future schema may reuse the
/// same field names with different meaning. Accepting any integer (which is all
/// the JSON-schema layer's `minimum: 1` does) would make that a SILENT misread
/// — we would render schema-2 data as though it were schema 1. A loud refusal
/// is the correct failure here; a silent misread is not.
const SUPPORTED_SCHEMA: i64 = 1;

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

/// Validate the optional slop-ferret sweep, if one is registered.
pub fn validate_slop_ferret(data_dir: &Path, errors: &mut Vec<String>) {
    let manifest = match read_json(&data_dir.join("manifest.json"), errors) {
        Some(v) => v,
        None => return,
    };
    let rel = match manifest
        .get("files")
        .and_then(|f| f.get("slopFerret"))
        .and_then(Value::as_str)
    {
        Some(r) => r,
        None => return, // optional + unregistered => pass
    };
    let doc = match read_json(&data_dir.join(rel), errors) {
        Some(v) => v,
        None => return,
    };

    // --- schema-version gating (single source of the version error) ---------
    let schema = doc.get("schema").and_then(Value::as_i64).unwrap_or(0);
    if schema != SUPPORTED_SCHEMA {
        errors.push(format!(
            "slop-ferret schema {schema} is unsupported; this build consumes schema {SUPPORTED_SCHEMA}"
        ));
        return; // every check below assumes schema-1 field meanings
    }

    validate_counts(&doc, errors);
}

/// Reconcile the sweep's self-reported counts against its findings array.
///
/// Split out so the reconciliation is testable without a filesystem.
pub fn validate_counts(doc: &Value, errors: &mut Vec<String>) {
    let findings = doc.get("findings").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);

    // `findings_verified` / `findings_suspected` are optional; when a sweep
    // states them, they are a claim about the array sitting next to them, and a
    // reader has no way to tell which one is lying. Check each independently,
    // so a sweep that reports only one still gets that one checked.
    let count_status = |want: &str| {
        findings
            .iter()
            .filter(|f| f.get("status").and_then(Value::as_str) == Some(want))
            .count() as i64
    };
    for (field, status) in [("findings_verified", "VERIFIED"), ("findings_suspected", "SUSPECTED")] {
        if let Some(stated) = doc.get(field).and_then(Value::as_i64) {
            let actual = count_status(status);
            if stated != actual {
                errors.push(format!(
                    "slop-ferret {field} is {stated} but {actual} finding(s) carry status \"{status}\""
                ));
            }
        }
    }

    // DELIBERATELY NOT CHECKED, and worth recording so nobody adds them back
    // on intuition the way I first did:
    //
    // - `accounting == "complete"` vs `unmatched_size > 0`. Looks like a
    //   contradiction, is not: `unmatched_size` is the count of unmatched
    //   HYPOTHESES from the plan (`plan.h_unmatched`), which is orthogonal to
    //   whether the file accounting closed. The shipped fixture legitimately
    //   carries `complete` with `unmatched_size: 1`; asserting otherwise
    //   rejected a valid sweep.
    // - `waived <= denominator`. Plausible, but `waived` comes from the
    //   attestation and `denominator` from `plan.production_total`, and nothing
    //   in the producer establishes that they are counted over the same set.
    //
    // The rule the two share: a cross-field invariant is only worth asserting
    // when the producer's own derivation shows the fields are commensurable.
    // Inventing one rejects valid documents, which is a worse failure than the
    // inconsistency it was guessing at.
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn finding(status: &str) -> Value {
        json!({
            "title": "t", "file": "f.go", "class": "H", "severity": "blocking", "status": status
        })
    }

    #[test]
    fn stated_counts_must_match_the_findings_actually_carried() {
        // WHY: the counts are what the viewer turns into a coverage claim. If
        // they can drift from the array, the panel renders an authoritative
        // number that nothing backs — the specific failure this tool exists to
        // prevent in other people's code.
        let doc = json!({
            "findings_verified": 3,               // claims 3
            "findings_suspected": 0,
            "findings": [finding("VERIFIED")],    // carries 1
        });
        let mut errors = Vec::new();
        validate_counts(&doc, &mut errors);
        assert!(
            errors.iter().any(|e| e.contains("findings_verified is 3 but 1")),
            "expected a verified-count mismatch; got {errors:?}"
        );
    }

    #[test]
    fn each_stated_count_is_checked_independently() {
        // A sweep may state only one of the two. Stating `suspected` correctly
        // must not excuse a wrong `verified`, and omitting a field must not be
        // read as a claim of zero.
        let doc = json!({
            "findings_suspected": 1,
            "findings": [finding("VERIFIED"), finding("SUSPECTED")],
        });
        let mut errors = Vec::new();
        validate_counts(&doc, &mut errors);
        assert!(errors.is_empty(), "omitted verified count must not be treated as 0: {errors:?}");
    }



    #[test]
    fn a_consistent_sweep_passes_cleanly() {
        let doc = json!({
            // `unmatched_size` deliberately non-zero alongside `accounting:
            // "complete"` — that combination is legitimate (see the
            // NOT-CHECKED note above) and this pins that we accept it.
            "denominator": 4, "waived": 1, "unmatched_size": 1, "accounting": "complete",
            "findings_verified": 1, "findings_suspected": 1,
            "findings": [finding("VERIFIED"), finding("SUSPECTED")],
        });
        let mut errors = Vec::new();
        validate_counts(&doc, &mut errors);
        assert!(errors.is_empty(), "consistent sweep must pass: {errors:?}");
    }
}
