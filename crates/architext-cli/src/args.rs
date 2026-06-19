//! Hand-rolled argv parser — exact port of `parseArgs` + `validateOptions` in
//! `src/adapters/cli/command-line.mjs`.
//!
//! Design rules:
//!  - Keep semantics byte-identical to the JS source.
//!  - Parse errors are `Err(String)` — callers print the message and exit(1).
//!  - No third-party parsing crates (they diverge on subtle JS behaviours).

use std::net::IpAddr;

/// All option fields from the JS `options` object in `parseArgs`.
#[derive(Debug, Clone)]
pub struct ParsedArgs {
    pub command: String,
    pub target: String,
    pub topic: String,
    pub yes: bool,
    pub quiet: bool,
    pub prompt: bool,
    pub foreground: bool,
    pub background: bool,
    pub serve_list: bool,
    pub serve_restart: bool,
    pub serve_instance: String,
    pub check_updates: bool,
    pub open: bool,
    pub no_open: bool,
    pub host: String,
    pub port: u32,
    pub serve_status: bool,
    pub serve_stop: bool,
    pub json: bool,
    pub dry_run: bool,
    pub force: bool,
    pub overwrite_data: bool,
    pub append_agents: bool,
    pub no_agents: bool,
    pub root_scripts: bool,
    pub no_root_scripts: bool,
    pub update_gitignore: bool,
    pub no_gitignore: bool,
    pub mode: String,
    pub out: String,
    pub skip_validate: bool,
    pub node_modules: bool,
    pub branch: String,
    pub branch_name: String,
}

const KNOWN_COMMANDS: &[&str] = &[
    "install", "upgrade", "sync", "migrate", "doctor", "status", "serve",
    "validate", "build", "prompt", "skill", "clean", "explain", "help", "version",
];

fn assert_serve_command(command: &str, arg: &str) -> Result<(), String> {
    if command != "serve" {
        Err(format!("{arg} is only valid for architext serve"))
    } else {
        Ok(())
    }
}

/// Port of JS `isLoopbackHost`.
pub fn is_loopback_host(host: &str) -> bool {
    // Strip [IPv6] brackets properly
    let normalized = {
        let lower = host.to_lowercase();
        if lower.starts_with('[') && lower.ends_with(']') {
            lower[1..lower.len() - 1].to_string()
        } else {
            lower
        }
    };

    if normalized == "localhost" || normalized == "::1" {
        return true;
    }
    if let Ok(IpAddr::V4(v4)) = normalized.parse::<IpAddr>() {
        return v4.octets()[0] == 127;
    }
    false
}

fn validate_options(opts: &ParsedArgs) -> Result<(), String> {
    if opts.command != "serve" {
        return Ok(());
    }
    if opts.foreground && opts.background {
        return Err("--foreground and --background cannot be used together".to_string());
    }
    if opts.open && opts.no_open {
        return Err("--open and --no-open cannot be used together".to_string());
    }
    let lifecycle_count = [
        opts.serve_status,
        opts.serve_stop,
        opts.serve_list,
        opts.serve_restart,
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    if lifecycle_count > 1 {
        return Err(
            "--status, --stop, --list, and --restart cannot be used together".to_string(),
        );
    }
    if lifecycle_count > 0
        && (opts.foreground || opts.background || opts.open || opts.no_open)
    {
        return Err(
            "--status, --stop, --list, and --restart cannot be combined with serve startup options"
                .to_string(),
        );
    }
    if !(opts.serve_instance.is_empty() || opts.serve_status || opts.serve_stop || opts.serve_list || opts.serve_restart)
    {
        return Err(
            "--instance requires --status, --stop, --list, or --restart".to_string(),
        );
    }
    if opts.host.is_empty() {
        return Err("--host requires a value".to_string());
    }
    if !is_loopback_host(&opts.host) {
        return Err(
            "--host must be a loopback address: localhost, 127.0.0.1, or ::1".to_string(),
        );
    }
    // port must be integer 0-65535
    if opts.port > 65535 {
        return Err("--port must be an integer between 0 and 65535".to_string());
    }
    Ok(())
}

/// Parse a slice of argv strings (already stripped of `argv[0..2]` in Node
/// terms, i.e. just the user-facing arguments).
pub fn parse_args(argv: &[String]) -> Result<ParsedArgs, String> {
    let first = argv.first().map(String::as_str).unwrap_or("");
    let has_command = !first.starts_with("--") && KNOWN_COMMANDS.contains(&first);
    let command = if has_command {
        first.to_string()
    } else {
        "sync".to_string()
    };
    let rest: &[String] = if has_command { &argv[1..] } else { argv };

    let mut opts = ParsedArgs {
        command,
        target: String::new(),
        topic: String::new(),
        yes: false,
        quiet: false,
        prompt: false,
        foreground: false,
        background: false,
        serve_list: false,
        serve_restart: false,
        serve_instance: String::new(),
        check_updates: false,
        open: false,
        no_open: false,
        host: "127.0.0.1".to_string(),
        port: 4317,
        serve_status: false,
        serve_stop: false,
        json: false,
        dry_run: false,
        force: false,
        overwrite_data: false,
        append_agents: false,
        no_agents: false,
        root_scripts: false,
        no_root_scripts: false,
        update_gitignore: false,
        no_gitignore: false,
        mode: "initial-buildout".to_string(),
        out: String::new(),
        skip_validate: false,
        node_modules: false,
        branch: String::new(),
        branch_name: String::new(),
    };

    let mut index = 0usize;
    while index < rest.len() {
        let arg = rest[index].as_str();
        match arg {
            "--target" => {
                index += 1;
                opts.target = rest.get(index).cloned().unwrap_or_default();
            }
            "--yes" | "-y" => opts.yes = true,
            "--quiet" => opts.quiet = true,
            "--prompt" => opts.prompt = true,
            "--foreground" => {
                assert_serve_command(&opts.command, arg)?;
                opts.foreground = true;
            }
            "--background" => {
                assert_serve_command(&opts.command, arg)?;
                opts.background = true;
            }
            "--list" => {
                opts.command = "serve".to_string();
                opts.serve_list = true;
            }
            "--instance" => {
                assert_serve_command(&opts.command, arg)?;
                index += 1;
                let value = rest.get(index).cloned().unwrap_or_default();
                if value.is_empty() {
                    return Err("--instance requires a value".to_string());
                }
                opts.serve_instance = value;
            }
            "--restart" | "--refresh" | "--update" => {
                assert_serve_command(&opts.command, arg)?;
                opts.serve_restart = true;
            }
            "--check-updates" => {
                opts.command = "version".to_string();
                opts.check_updates = true;
            }
            "--open" => {
                assert_serve_command(&opts.command, arg)?;
                opts.open = true;
            }
            "--no-open" => {
                assert_serve_command(&opts.command, arg)?;
                opts.no_open = true;
            }
            "--host" => {
                assert_serve_command(&opts.command, arg)?;
                index += 1;
                opts.host = rest.get(index).cloned().unwrap_or_default();
            }
            "--port" => {
                assert_serve_command(&opts.command, arg)?;
                index += 1;
                let raw = rest.get(index).cloned().unwrap_or_default();
                // JS: Number(raw) — NaN → 0, which becomes 0 u32; non-integer
                // values become NaN → 0 in JS (which passes the 0..=65535 check).
                // We reproduce by parsing as f64 then truncating.
                let num = raw.parse::<f64>().unwrap_or(f64::NAN);
                if num.is_nan() || num.fract() != 0.0 {
                    // JS Number() of non-numeric string → NaN; !Number.isInteger → error
                    opts.port = u32::MAX; // triggers validation error below
                } else {
                    opts.port = num as i64 as u32;
                }
            }
            "--status" => {
                assert_serve_command(&opts.command, arg)?;
                opts.serve_status = true;
            }
            "--stop" => {
                assert_serve_command(&opts.command, arg)?;
                opts.serve_stop = true;
            }
            "--json" => opts.json = true,
            "--dry-run" => opts.dry_run = true,
            "--force" => opts.force = true,
            "--overwrite-data" => opts.overwrite_data = true,
            "--append-agents" => opts.append_agents = true,
            "--no-agents" => opts.no_agents = true,
            "--root-scripts" => opts.root_scripts = true,
            "--no-root-scripts" => opts.no_root_scripts = true,
            "--update-gitignore" => opts.update_gitignore = true,
            "--no-gitignore" => opts.no_gitignore = true,
            "--mode" => {
                index += 1;
                opts.mode = rest.get(index).cloned().unwrap_or_default();
            }
            "--out" => {
                index += 1;
                opts.out = rest.get(index).cloned().unwrap_or_default();
            }
            "--skip-validate" => opts.skip_validate = true,
            "--node-modules" => opts.node_modules = true,
            "--branch" => {
                index += 1;
                opts.branch = rest.get(index).cloned().unwrap_or_default();
            }
            "--branch-name" => {
                index += 1;
                opts.branch_name = rest.get(index).cloned().unwrap_or_default();
            }
            "--help" | "-h" => opts.command = "help".to_string(),
            "--version" | "-v" => opts.command = "version".to_string(),
            _ => {
                // explain topic (positional, first non-flag after command)
                if opts.command == "explain" && opts.topic.is_empty() {
                    opts.topic = arg.to_string();
                } else if opts.target.is_empty() {
                    opts.target = arg.to_string();
                } else {
                    return Err(format!("Unknown argument: {arg}"));
                }
            }
        }
        index += 1;
    }

    validate_options(&opts)?;
    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        if s.is_empty() {
            return vec![];
        }
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn default_command_is_sync() {
        let opts = parse_args(&[]).unwrap();
        assert_eq!(opts.command, "sync");
    }

    #[test]
    fn version_flag() {
        assert_eq!(parse_args(&args("--version")).unwrap().command, "version");
        assert_eq!(parse_args(&args("-v")).unwrap().command, "version");
    }

    #[test]
    fn help_flag() {
        assert_eq!(parse_args(&args("--help")).unwrap().command, "help");
        assert_eq!(parse_args(&args("-h")).unwrap().command, "help");
    }

    #[test]
    fn known_command_consumed() {
        let opts = parse_args(&args("validate /tmp/foo")).unwrap();
        assert_eq!(opts.command, "validate");
        assert_eq!(opts.target, "/tmp/foo");
    }

    #[test]
    fn unknown_arg_errors() {
        // JS: catch-all puts first unknown positional into `target`.
        // A second unknown positional (when target is already set) → error.
        let err = parse_args(&args("sync . --bogus")).unwrap_err();
        assert!(err.contains("Unknown argument: --bogus"), "got: {err}");
    }

    #[test]
    fn foreground_non_serve_errors() {
        let err = parse_args(&args("validate --foreground")).unwrap_err();
        assert!(err.contains("is only valid for architext serve"), "got: {err}");
    }

    #[test]
    fn foreground_background_mutual_exclusion() {
        let err = parse_args(&args("serve --foreground --background")).unwrap_err();
        assert!(err.contains("--foreground and --background cannot be used together"), "got: {err}");
    }

    #[test]
    fn mode_default_initial_buildout() {
        let opts = parse_args(&args("prompt")).unwrap();
        assert_eq!(opts.mode, "initial-buildout");
    }

    #[test]
    fn mode_explicit() {
        let opts = parse_args(&args("prompt --mode architecture-change")).unwrap();
        assert_eq!(opts.mode, "architecture-change");
    }

    #[test]
    fn json_flag() {
        let opts = parse_args(&args("status . --json")).unwrap();
        assert!(opts.json);
    }

    #[test]
    fn explain_topic_positional() {
        let opts = parse_args(&args("explain nodes")).unwrap();
        assert_eq!(opts.topic, "nodes");
        assert_eq!(opts.command, "explain");
    }

    #[test]
    fn check_updates_sets_version_command() {
        let opts = parse_args(&args("--check-updates")).unwrap();
        assert_eq!(opts.command, "version");
        assert!(opts.check_updates);
    }

    #[test]
    fn list_flag_sets_serve_command() {
        let opts = parse_args(&args("--list")).unwrap();
        assert_eq!(opts.command, "serve");
        assert!(opts.serve_list);
    }

    #[test]
    fn host_loopback_validation() {
        let err = parse_args(&args("serve --host 0.0.0.0")).unwrap_err();
        assert!(err.contains("loopback"), "got: {err}");
    }

    #[test]
    fn is_loopback_host_cases() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("127.0.0.2"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.1"));
    }
}
