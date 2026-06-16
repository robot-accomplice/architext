//! Thin CLI shim for the `jsonwrite-parity-rust.mjs` differential gate.
//!
//! Usage: jsonwrite_dump <file.json>
//!
//! Reads the file, parses to `serde_json::Value` (preserve_order keeps key
//! order identical to the input), calls `write_json_string`, and writes the
//! result to stdout with NO extra newline — `write_json_string` already
//! appends the trailing `\n` that matches JS `writeJson`.

use std::{env, fs, io::Write, process};

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: jsonwrite_dump <file.json>");
        process::exit(1);
    });

    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Failed to read {path}: {e}");
        process::exit(1);
    });

    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("Failed to parse {path}: {e}");
        process::exit(1);
    });

    let result = architext_core::json_write::write_json_string(&value);

    // Use print! / stdout directly so we write exactly the bytes returned —
    // no extra newline beyond the one already embedded in `result`.
    std::io::stdout()
        .write_all(result.as_bytes())
        .unwrap_or_else(|e| {
            eprintln!("Failed to write output: {e}");
            process::exit(1);
        });
}
