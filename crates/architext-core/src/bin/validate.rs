/// Thin CLI shim for the validation conformance harness.
///
/// Usage: validate <data-dir> [schema-dir]
///
/// Schema dir resolution (first match wins):
///   1. Second positional argument.
///   2. `ARCHITEXT_SCHEMA_DIR` environment variable.
///   3. `viewer/schema/` relative to the current working directory
///      (the conformance harness runs `cargo run` with `cwd = repoRoot`,
///      so this resolves correctly without any explicit argument).
///
/// Output: JSON {"ok": bool, "errors": [string, ...]}
/// Exit code: 0 always (the harness interprets ok/errors, not exit code).
fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_default();

    let schema_dir = args
        .next()
        .or_else(|| std::env::var("ARCHITEXT_SCHEMA_DIR").ok())
        .unwrap_or_else(|| "viewer/schema".to_string());

    let outcome = architext_core::validate_data_dir(
        std::path::Path::new(&dir),
        std::path::Path::new(&schema_dir),
    );

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
