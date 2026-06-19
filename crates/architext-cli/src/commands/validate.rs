//! `validate [path]` — port of the JS `validate` command handler and
//! `validateTarget` helper in `src/adapters/cli/architext-cli.mjs`.
//!
//! JS output contract (via `validateTarget` → subprocess → `tryRun`):
//!   success stdout: "Architext validation passed."
//!   failure stdout+stderr:
//!     "Architext validation failed:\n- err1\n- err2"
//! The JS handler does `console.log(validation.output)` then
//! `process.exit(1)` on failure.

use std::path::Path;
use std::process;

use architext_core::validate_data_dir;

fn schema_dir() -> std::path::PathBuf {
    if let Ok(env_dir) = std::env::var("ARCHITEXT_SCHEMA_DIR") {
        return std::path::PathBuf::from(env_dir);
    }
    // During `cargo run` the cwd is the repo root; viewer/schema is correct.
    std::path::PathBuf::from("viewer").join("schema")
}

/// Run validation.  Prints to stdout; calls `process::exit(1)` on failure.
pub fn run(target: &Path) {
    let data_dir = target.join("docs").join("architext").join("data");
    if !data_dir.join("manifest.json").exists() {
        // JS validateTarget returns { ok: false, output: "Architext data is not installed at <dir>" }
        // then the handler does console.log(output) and process.exit(1).
        println!("Architext data is not installed at {}", data_dir.display());
        process::exit(1);
    }

    let outcome = validate_data_dir(&data_dir, &schema_dir());
    if outcome.ok {
        println!("Architext validation passed.");
    } else {
        // JS validator writes to stderr; tryRun combines stdout+stderr.
        // We write to stdout because the JS handler does console.log() (stdout).
        println!("Architext validation failed:");
        for error in &outcome.errors {
            println!("- {error}");
        }
        process::exit(1);
    }
}
