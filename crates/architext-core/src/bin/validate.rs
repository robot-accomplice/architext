/// Thin CLI shim for the validation conformance harness.
///
/// Usage: validate <data-dir>
/// Output: JSON {"ok": bool, "errors": [string, ...]}
/// Exit code: 0 always (the harness interprets ok/errors, not exit code).
fn main() {
    let dir = std::env::args().nth(1).unwrap_or_default();
    let outcome = architext_core::validate_data_dir(std::path::Path::new(&dir));
    // Minimal hand-rolled JSON — avoids adding a heavy formatter dep for a stub.
    let errors_json: Vec<String> = outcome
        .errors
        .iter()
        .map(|e| format!("\"{}\"", e.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    println!(
        "{{\"ok\":{},\"errors\":[{}]}}",
        outcome.ok,
        errors_json.join(",")
    );
}
