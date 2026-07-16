//! `doctor [path] [--json] [--yes] [--dry-run] [--skip-validate]` — port of
//! `runDoctor(target, options, version)` in `src/adapters/cli/architext-cli.mjs`
//! (~line 824).
//!
//! Flow:
//!   1. `collect_status(target, version, runValidation=true)`.
//!   2. `--json` → print the status object and return (no writes).
//!   3. print verbose status.
//!   4. if not installed / needs migration → "Next: architext sync".
//!   5. if no doctor repairs → "Next: …prompt" (validation failed) or
//!      "Next: architext serve".
//!   6. `--dry-run` → "Dry run: no doctor repairs applied." and return.
//!   7. without `--yes`, refuse when stdin is not a TTY (repairs mutate
//!      tracked files; a non-interactive shell must opt in explicitly) and
//!      never treat EOF at the prompt as consent. Once confirmed (via
//!      `--yes` or an interactive prompt) apply repairs (REUSING core's
//!      `apply_doctor_repairs`), print applied list, re-validate, exit 1 if the
//!      post-repair validation fails.
//!
//! Doctor reuses `architext_core::status::collect_status` and
//! `architext_core::domain::doctor_repairs::apply_doctor_repairs`; it does not
//! re-port detection or repair logic.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process;

use serde_json::{json, Value};

use architext_core::domain::doctor_repairs::apply_doctor_repairs;
use architext_core::status::collect_status;

use crate::commands::status::format_status_lines_verbose;

/// Build a `{ ok, output }` validation object matching JS `validateTarget`.
/// JS shells out to the validator and captures combined stdout/stderr; the Rust
/// validator wording is the same as `commands::validate`.
fn validate_target(target: &Path) -> Value {
    let data_dir = target.join("docs").join("architext").join("data");
    if !data_dir.join("manifest.json").exists() {
        return json!({ "ok": false, "output": format!("Architext data is not installed at {}", data_dir.display()) });
    }
    let schema_dir = schema_dir();
    let outcome = architext_core::validate_data_dir(&data_dir, &schema_dir);
    if outcome.ok {
        json!({ "ok": true, "output": "Architext validation passed." })
    } else {
        let mut output = String::from("Architext validation failed:");
        for err in &outcome.errors {
            output.push_str(&format!("\n- {err}"));
        }
        json!({ "ok": false, "output": output })
    }
}

fn schema_dir() -> std::path::PathBuf {
    if let Ok(env_dir) = std::env::var("ARCHITEXT_SCHEMA_DIR") {
        return std::path::PathBuf::from(env_dir);
    }
    std::path::PathBuf::from("viewer").join("schema")
}

/// Inject the `{ ok, output }` validation object into a status produced by
/// `collect_status` (which only carries `{ ok }`), so verbose printing and
/// `--json` match the JS shape.
fn status_with_validation(target: &Path, version: &str) -> Value {
    let mut status = collect_status(target, version, true);
    // Replace the bare `{ ok }` validation with `{ ok, output }`.
    if status.get("installed").and_then(|v| v.as_bool()).unwrap_or(false)
        || status["validation"].get("ok").is_some()
    {
        status["validation"] = validate_target(target);
    }
    status
}

fn prompt_yes_no(question: &str, default_value: bool) -> bool {
    let suffix = if default_value { "Y/n" } else { "y/N" };
    print!("{question} [{suffix}] ");
    let _ = io::stdout().flush();
    read_yes_no_answer(io::stdin().lock(), default_value)
}

fn read_yes_no_answer(mut input: impl io::BufRead, default_value: bool) -> bool {
    let mut line = String::new();
    match input.read_line(&mut line) {
        // EOF or a read error is not consent; only a typed answer (or a bare
        // Enter from a live terminal) may accept a mutating repair.
        Ok(0) | Err(_) => return false,
        Ok(_) => {}
    }
    let answer = line.trim().to_lowercase();
    if answer.is_empty() {
        return default_value;
    }
    answer == "y" || answer == "yes"
}

/// Run `doctor`. Mirrors JS `runDoctor`.
pub fn run(target: &Path, opts: &crate::args::ParsedArgs, version: &str) {
    let status = status_with_validation(target, version);

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&status).unwrap());
        return;
    }

    for line in format_status_lines_verbose(&status) {
        println!("{line}");
    }

    let installed = status["installed"].as_bool().unwrap_or(false);
    let needs_migration = status["needsMigration"].as_bool().unwrap_or(false);
    if !installed || needs_migration {
        println!("Next: architext sync");
        return;
    }

    let repairs = status["doctorRepairs"].as_array().cloned().unwrap_or_default();
    if repairs.is_empty() {
        let validation_ok = status["validation"]["ok"].as_bool().unwrap_or(false);
        if status.get("validation").map(|v| !v.is_null()).unwrap_or(false) && !validation_ok {
            println!("Next: architext prompt --mode repair-validation");
            return;
        }
        println!("Next: architext serve");
        return;
    }

    if opts.dry_run {
        println!("Dry run: no doctor repairs applied.");
        return;
    }

    if !opts.yes && !io::stdin().is_terminal() {
        println!("No doctor repairs applied: stdin is not a terminal.");
        println!("Re-run with --yes to apply repairs, or --dry-run to preview them.");
        return;
    }

    let apply = opts.yes || prompt_yes_no("Apply deterministic doctor repairs?", true);
    if !apply {
        println!("No doctor repairs applied.");
        return;
    }

    let applied = apply_doctor_repairs(target, &status, false, false);
    println!("Applied doctor repairs:");
    for repair in &applied {
        println!("- {}: {}", repair.file, repair.summary);
    }

    let validation = if opts.skip_validate {
        json!({ "ok": true, "output": "Validation skipped." })
    } else {
        validate_target(target)
    };
    println!("{}", validation["output"].as_str().unwrap_or(""));
    if !validation["ok"].as_bool().unwrap_or(false) {
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::read_yes_no_answer;

    #[test]
    fn eof_is_never_consent() {
        // A non-interactive stdin (EOF, no input at all) must decline even when
        // the prompt default is "yes" — doctor repairs mutate tracked files.
        assert!(!read_yes_no_answer(std::io::empty(), true));
    }

    #[test]
    fn explicit_answers_are_respected() {
        assert!(read_yes_no_answer("y\n".as_bytes(), false));
        assert!(read_yes_no_answer("yes\n".as_bytes(), false));
        assert!(!read_yes_no_answer("n\n".as_bytes(), true));
        assert!(!read_yes_no_answer("nonsense\n".as_bytes(), true));
    }

    #[test]
    fn bare_enter_takes_the_default() {
        assert!(read_yes_no_answer("\n".as_bytes(), true));
        assert!(!read_yes_no_answer("\n".as_bytes(), false));
    }
}
