//! `architext` binary — argv adapter for the Rust CLI.
//!
//! Mirrors the JS `main()` in `src/adapters/cli/architext-cli.mjs` and
//! the `tools/architext-adopt.mjs` bin shim.
//!
//! Architecture: this file is pure routing.  Business logic lives in
//! `architext_core` and the `commands::*` modules.

mod args;
mod commands;
mod usage;

use std::process;

fn package_version() -> &'static str {
    // Stamped by build.rs from the repo's package.json (the product version,
    // bumped by the release process); falls back to CARGO_PKG_VERSION there.
    env!("ARCHITEXT_VERSION")
}

fn resolve_target(raw: &str) -> std::path::PathBuf {
    let base = if raw.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        // JS: path.resolve(options.target || process.cwd())
        let p = std::path::Path::new(raw);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(p)
        }
    };
    // Match JS `path.resolve`: lexically collapse `.`/`..`, WITHOUT resolving
    // symlinks or requiring the path to exist. (canonicalize() was wrong — it
    // follows symlinks, so it printed /private/var for /var and would resolve
    // any symlinked repo path, diverging from Node which prints the path as-given.)
    lexical_normalize(&base)
}

/// Lexically normalize an absolute path the way Node's `path.resolve` does:
/// drop `.` segments, pop on `..`, no filesystem access, no symlink resolution.
fn lexical_normalize(p: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    let opts = match args::parse_args(&raw_args) {
        Ok(o) => o,
        Err(msg) => {
            // JS: the main().catch handler does console.error(error.message); process.exit(1)
            eprintln!("{msg}");
            process::exit(1);
        }
    };

    // JS: if (options.command === "help") { console.log(usage()); return; }
    if opts.command == "help" {
        println!("{}", usage::usage());
        return;
    }

    let version = package_version();

    // JS: if (options.checkUpdates) { … } — not in this slice (side-effecting)
    if opts.check_updates {
        eprintln!("--check-updates is not yet implemented in the Rust CLI");
        process::exit(1);
    }

    // JS: if (options.command === "version") { console.log(version); return; }
    if opts.command == "version" {
        println!("{version}");
        return;
    }

    // Commands that do NOT need a target directory check
    if opts.command == "skill" {
        commands::skill::run();
        return;
    }

    if opts.command == "explain" {
        commands::explain::run(&opts.topic);
        return;
    }

    // All other commands resolve + assert the target
    let target = resolve_target(&opts.target);

    // JS: if (!["explain", "skill"].includes(options.command)) await assertTarget(target)
    // assertTarget checks it's a directory.
    if !target.is_dir() {
        eprintln!(
            "Target is not a directory: {}",
            target.display()
        );
        process::exit(1);
    }

    match opts.command.as_str() {
        "validate" => commands::validate::run(&target),
        "status" => commands::status::run(&target, opts.json, version),
        "prompt" => commands::prompt::run(&target, &opts.mode),
        "build" => commands::build::run(&target, &opts.out),
        "clean" => commands::clean::run(&target, opts.node_modules, opts.dry_run),

        "sync" | "install" | "upgrade" | "migrate" => {
            commands::sync::run(&target, &opts, version);
        }
        "serve" => commands::serve::run(&target, &opts, version),
        "doctor" => commands::doctor::run(&target, &opts, version),
        unknown => {
            // JS: routeCommand throws `Unknown command: ${options.command}`
            eprintln!("Unknown command: {unknown}");
            process::exit(1);
        }
    }
}
