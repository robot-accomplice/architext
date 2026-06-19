//! `status_dump <target> [--version <v>] [--validate]`
//!
//! Runs `collect_status` and prints the result as JSON to stdout.
//! Used by the status-parity-rust.mjs gate and by future CLI/serve layers.
//!
//! Usage:
//!   status_dump <target>
//!   status_dump <target> --version 1.7.0
//!   status_dump <target> --version 1.7.0 --validate
//!
//! Exit code: always 0 (the harness reads the JSON, not the exit code).

use std::{env, path::Path, process};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("Usage: status_dump <target> [--version <v>] [--validate]");
        process::exit(1);
    }

    let target_str = &args[0];
    let mut version = "0.0.0".to_string();
    let mut run_validation = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--version" => {
                i += 1;
                if i < args.len() {
                    version = args[i].clone();
                }
            }
            "--validate" => {
                run_validation = true;
            }
            _ => {}
        }
        i += 1;
    }

    let target = Path::new(target_str);
    if !target.is_dir() {
        eprintln!("Target is not a directory: {target_str}");
        process::exit(1);
    }

    let status = architext_core::status::collect_status(target, &version, run_validation);
    println!("{}", serde_json::to_string_pretty(&status).unwrap());
}
