//! `serve` lifecycle command — port of `runServeLifecycle` and the surrounding
//! helpers in `src/adapters/cli/serve-lifecycle.mjs`.
//!
//! Modes: `--foreground` (run the server in-process until ^C), `--background`
//! (spawn a detached `architext serve <target> --foreground …`, wait for the
//! state file + reachability, return), `--list`, `--status`, `--stop`,
//! `--restart`/`--refresh` (sync then stop+restart on the SAME port).
//!
//! The HTTP server itself is `architext-serve` — this module reuses
//! `architext_serve::{bind_listener, build_router, AppState}` and the plan farm;
//! it does NOT re-implement the server.

pub mod helpers;
pub mod process_control;
pub mod state;

use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::args::ParsedArgs;

use helpers::{browser_open_command, current_platform, is_loopback_serve_url, serve_url};
use process_control::{pid_exists, stop_serve_process_default};
use state::{
    read_serve_state, read_serve_state_by_id, remove_serve_state, remove_serve_state_by_id,
    remove_serve_state_if_owned, serve_log_path, serve_runtime_dir, serve_state_key,
    write_serve_state,
};

// Startup/poll constants (mirror serve-lifecycle.mjs:28-30).
const SERVE_STARTUP_TIMEOUT_MS: u64 = 15000;
const SERVE_STARTUP_POLL_MS: u64 = 100;

// ─── ISO-8601 timestamp (mirrors `new Date().toISOString()`) ──────────────────

fn now_iso8601() -> String {
    // Match JS `new Date().toISOString()` → "YYYY-MM-DDTHH:MM:SS.sssZ".
    // We only need a UTC ISO string; the exact instant is non-deterministic and
    // normalized in the parity gate (documented), same as every other timestamp.
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    // Civil-from-days (Howard Hinnant's algorithm) for UTC date.
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
}

// ─── reachability ─────────────────────────────────────────────────────────────

/// Port of `urlReachable(url)`: a loopback `http:` GET that returns `ok`
/// (2xx). Uses a blocking TCP HTTP/1.0 GET to avoid pulling an HTTP client.
fn url_reachable(url: &str) -> bool {
    if !is_loopback_serve_url(url) {
        return false;
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let path = if authority_end < rest.len() { &rest[authority_end..] } else { "/" };
    http_get_ok(authority, path)
}

/// Minimal blocking HTTP/1.1 GET — returns true iff the status line is 2xx.
/// Reads in a loop until the status line (first CRLF) is available or the read
/// times out, so a partial first read does not spuriously report "down".
fn http_get_ok(authority: &str, path: &str) -> bool {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::net::ToSocketAddrs;

    // Resolve with a connect timeout so a wrong/closed port fails fast.
    let Ok(mut addrs) = authority.to_socket_addrs() else { return false };
    let Some(addr) = addrs.next() else { return false };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(1000)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));
    // HTTP/1.1 with Connection: close so the server closes after one response.
    let req = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut acc = Vec::with_capacity(256);
    let mut chunk = [0u8; 256];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                acc.extend_from_slice(&chunk[..n]);
                if acc.windows(2).any(|w| w == b"\r\n") || acc.len() > 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let head = String::from_utf8_lossy(&acc);
    let Some(status_line) = head.lines().next() else { return false };
    // "HTTP/1.1 200 OK" → the second whitespace token is the code.
    status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .map(|c| (200..300).contains(&c))
        .unwrap_or(false)
}

/// Port of `staleServeState`: stale if no state, pid dead, or url unreachable.
fn stale_serve_state(state: &Value) -> bool {
    let pid = state["pid"].as_i64().unwrap_or(0);
    if !pid_exists(pid) {
        return true;
    }
    let url = state["url"].as_str().unwrap_or("");
    !url_reachable(url)
}

// ─── instance resolution ──────────────────────────────────────────────────────

fn read_serve_instances(cleanup_stale: bool) -> Vec<Value> {
    let dir = serve_runtime_dir();
    let mut instances = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return instances;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let id = name[..name.len() - 5].to_string();
        let Some(state) = read_serve_state_by_id(&id) else {
            continue;
        };
        if stale_serve_state(&state) {
            if cleanup_stale {
                remove_serve_state_by_id(&id);
            }
            continue;
        }
        let mut obj = state;
        obj["id"] = json!(id);
        obj["status"] = json!("running");
        instances.push(obj);
    }
    instances.sort_by(|a, b| {
        let sa = a["startedAt"].as_str().unwrap_or("");
        let sb = b["startedAt"].as_str().unwrap_or("");
        sa.cmp(sb).then_with(|| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")))
    });
    instances
}

fn known_instance_error(id: &str, instances: &[Value]) -> String {
    let known = if instances.is_empty() {
        " No running instances are recorded.".to_string()
    } else {
        let ids: Vec<&str> = instances.iter().filter_map(|i| i["id"].as_str()).collect();
        format!(" Known instances: {}", ids.join(", "))
    };
    format!("Unknown Architext serve instance: {id}.{known}")
}

/// Port of `resolveInstance`. Returns `Ok(Some(state))`, `Ok(None)`, or
/// `Err(message)` for an unknown explicit `--instance`.
fn resolve_instance(opts: &ParsedArgs, target: &Path) -> Result<Option<Value>, String> {
    if !opts.serve_instance.is_empty() {
        let instances = read_serve_instances(true);
        let found = read_serve_state_by_id(&opts.serve_instance)
            .and(instances.iter().find(|c| c["id"].as_str() == Some(&opts.serve_instance)).cloned());
        match found {
            Some(instance) => Ok(Some(instance)),
            None => Err(known_instance_error(&opts.serve_instance, &instances)),
        }
    } else {
        match read_serve_state(target) {
            None => Ok(None),
            Some(state) => {
                if stale_serve_state(&state) {
                    remove_serve_state(target);
                    Ok(None)
                } else {
                    let mut obj = state;
                    obj["id"] = json!(serve_state_key(target));
                    obj["status"] = json!("running");
                    Ok(Some(obj))
                }
            }
        }
    }
}

// ─── browser launch ───────────────────────────────────────────────────────────

fn open_system_browser(url: &str) {
    let Some(cmd) = browser_open_command(current_platform(), url) else {
        eprintln!("Browser launch failed: No browser launcher is configured for {}", current_platform());
        return;
    };
    let result = Command::new(&cmd.command)
        .args(&cmd.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(e) = result {
        eprintln!("Browser launch failed: {e}");
    }
}

fn maybe_open(opts: &ParsedArgs, url: &str) {
    if opts.open && !opts.no_open {
        open_system_browser(url);
    }
}

// ─── viewer dist resolution ───────────────────────────────────────────────────

/// Resolve the viewer dist directory. The Rust serve path now prefers the
/// Trunk-built Leptos viewer (`crates/architext-viewer/dist`); the legacy React
/// `viewer/dist` is kept only as a transition fallback (its removal is Phase 3).
/// The `ARCHITEXT_VIEWER_DIST` env override still wins for explicit control.
///
/// For each candidate we check both the current working directory (cargo run
/// from the repo root) and the compile-time repo anchor (installed binary).
fn viewer_dist_dir() -> PathBuf {
    if let Ok(d) = std::env::var("ARCHITEXT_VIEWER_DIST") {
        return PathBuf::from(d);
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from);

    // Ordered candidates: Trunk dist first, legacy React dist as fallback.
    let trunk_rel = PathBuf::from("crates").join("architext-viewer").join("dist");
    let react_rel = PathBuf::from("viewer").join("dist");
    let candidates = [trunk_rel.clone(), react_rel.clone()];

    for rel in &candidates {
        // cwd-relative (cargo from repo root).
        if rel.join("index.html").exists() {
            return rel.clone();
        }
        // repo-anchored (installed / run from elsewhere).
        if let Some(root) = &repo_root {
            let anchored = root.join(rel);
            if anchored.join("index.html").exists() {
                return anchored;
            }
        }
    }

    // Nothing built yet: return the Trunk anchor so the "missing assets" error
    // points at the canonical 1.7.0 location.
    repo_root
        .map(|root| root.join(&trunk_rel))
        .unwrap_or(trunk_rel)
}

fn data_dir_for(target: &Path) -> PathBuf {
    target.join("docs").join("architext").join("data")
}

// ─── foreground ───────────────────────────────────────────────────────────────

/// Run the server in-process until ^C. Binds first (to learn the port), writes
/// the state file, then serves. Mirrors `serveForeground`.
fn serve_foreground(target: &Path, opts: &ParsedArgs) {
    use architext_serve::farm_state;
    use architext_serve::security::generate_mutation_token;
    use architext_serve::{build_router, write_txn, AppState};
    use std::sync::Arc;

    let dist_dir = viewer_dist_dir();
    if !dist_dir.join("index.html").exists() {
        eprintln!("Package viewer assets are missing. Run npm run build before serving Architext.");
        process::exit(1);
    }

    let host = opts.host.clone();
    let requested_port = opts.port as u16;

    let (listener, bound_port) = match architext_serve::bind_listener(&host, requested_port) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Architext serve could not bind {host}:{requested_port}: {e}");
            process::exit(1);
        }
    };

    let url = serve_url(&host, bound_port);
    let state = json!({
        "target": target.to_string_lossy(),
        "pid": std::process::id() as i64,
        "host": host,
        "port": bound_port,
        "url": url,
        "mode": "foreground",
        "startedAt": now_iso8601(),
    });
    if let Err(e) = write_serve_state(target, &state) {
        eprintln!("Failed to record serve state: {e}");
    }

    let data_dir = data_dir_for(target);
    let farm = farm_state::build_farm(&data_dir);
    let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join("viewer").join("schema"))
        .unwrap_or_else(|| PathBuf::from("viewer/schema"));

    let hub_data_dir = data_dir.clone();
    let hub_schema_dir = schema_dir.clone();
    let app_state = AppState {
        data_dir,
        dist_dir,
        mutation_token: Arc::new(generate_mutation_token()),
        target_dir: target.to_path_buf(),
        cli_version: Arc::new(env!("CARGO_PKG_VERSION").to_string()),
        schema_dir,
        write_lock: write_txn::new_write_lock(),
    };

    println!("Serving Architext for {}", target.display());
    println!("Open {url}");
    maybe_open(opts, &url);

    // Run the server on a tokio runtime until SIGINT/SIGTERM.
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    let target_owned = target.to_path_buf();
    let pid = std::process::id() as i64;
    runtime.block_on(async move {
        // The data-watch hub spawns a tokio task, so it must start inside the
        // runtime. A start failure is non-fatal — live-reload is simply off.
        let hub = match architext_serve::watch_hub::start_watch_hub(hub_data_dir, hub_schema_dir) {
            Ok(hub) => Some(hub),
            Err(e) => {
                eprintln!("data-watch hub disabled (live-reload off): {e}");
                None
            }
        };
        let router = build_router(app_state, farm, hub);

        // tokio requires the std listener to be non-blocking before from_std.
        listener.set_nonblocking(true).expect("set_nonblocking");
        let listener = tokio::net::TcpListener::from_std(listener).expect("from_std");
        let server = axum::serve(listener, router);
        let result = tokio::select! {
            r = server => r,
            _ = shutdown_signal() => Ok(()),
        };
        // Clean up our own state on exit (mirrors server.once("close")).
        remove_serve_state_if_owned(&target_owned, pid, "foreground");
        if let Err(e) = result {
            eprintln!("architext serve error: {e}");
        }
    });
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

// ─── background ───────────────────────────────────────────────────────────────

fn self_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("architext"))
}

/// Wait for the spawned child to publish reachable state (mirrors
/// `waitForChildServeState`).
fn wait_for_child_serve_state(target: &Path, pid: i64, timeout_ms: u64) -> Option<Value> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(timeout_ms) {
        if let Some(state) = read_serve_state(target) {
            if state["pid"].as_i64() == Some(pid) {
                let url = state["url"].as_str().unwrap_or("");
                if url_reachable(url) {
                    return Some(state);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(SERVE_STARTUP_POLL_MS));
    }
    None
}

/// Port of `serveBackground`: spawn a detached `serve --foreground` child, wait
/// for it to become reachable, write the authoritative background state.
fn serve_background(target: &Path, opts: &ParsedArgs) -> Result<(), String> {
    if let Some(existing) = read_serve_state(target) {
        if !stale_serve_state(&existing) {
            println!("Architext is already serving {}", existing["target"].as_str().unwrap_or(""));
            println!("Open {}", existing["url"].as_str().unwrap_or(""));
            maybe_open(opts, existing["url"].as_str().unwrap_or(""));
            return Ok(());
        }
        remove_serve_state(target);
    }

    let _ = std::fs::create_dir_all(serve_runtime_dir());
    let log_path = serve_log_path(target);
    let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log_path);
    let stdout = log_file
        .as_ref()
        .ok()
        .and_then(|f| f.try_clone().ok())
        .map(Stdio::from)
        .unwrap_or_else(Stdio::null);
    let stderr = log_file.ok().map(Stdio::from).unwrap_or_else(Stdio::null);

    let mut command = Command::new(self_exe());
    command
        .arg("serve")
        .arg(target.as_os_str())
        .arg("--foreground")
        .arg("--host")
        .arg(&opts.host)
        .arg("--port")
        .arg(opts.port.to_string())
        .arg("--no-open")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);

    // Detach into a new session so the child outlives this parent process —
    // mirrors JS `spawn(..., { detached: true })` + `child.unref()`. Without
    // this the child shares our session and dies when we exit, so the background
    // server would vanish the moment the `serve --background` command returns.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe and only detaches the child.
        unsafe {
            command.pre_exec(|| {
                // New session + process group so the detached server is insulated
                // from the spawning parent's controlling terminal going away —
                // matches JS `spawn(..., { detached: true })`. setsid making the
                // child a session leader (no controlling tty) means it will not
                // receive the terminal's SIGHUP. EPERM (already a leader) is
                // harmless and ignored.
                let _ = nix::unistd::setsid();
                Ok(())
            });
        }
    }

    let child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn Architext background serve: {e}"))?;
    let child_pid = child.id() as i64;
    // Detach: we do not wait() on the child (it outlives us).
    drop(child);

    let child_state = wait_for_child_serve_state(target, child_pid, SERVE_STARTUP_TIMEOUT_MS);
    let Some(child_state) = child_state else {
        if pid_exists(child_pid) {
            stop_serve_process_default(child_pid);
        }
        return Err(format!(
            "Architext background serve did not become reachable at {}. Check {}",
            serve_url(&opts.host, opts.port as u16),
            log_path.display()
        ));
    };

    let state = json!({
        "target": target.to_string_lossy(),
        "pid": child_pid,
        "host": child_state["host"].clone(),
        "port": child_state["port"].clone(),
        "url": child_state["url"].clone(),
        "logPath": log_path.to_string_lossy(),
        "mode": "background",
        "startedAt": now_iso8601(),
    });
    write_serve_state(target, &state).map_err(|e| format!("Failed to record serve state: {e}"))?;

    println!("Serving Architext for {} in the background", target.display());
    println!("Open {}", child_state["url"].as_str().unwrap_or(""));
    maybe_open(opts, child_state["url"].as_str().unwrap_or(""));
    Ok(())
}

// ─── status / list / stop / restart ─────────────────────────────────────────────

fn serve_status(target: &Path, opts: &ParsedArgs) -> Result<(), String> {
    let state = resolve_instance(opts, target)?;
    let Some(state) = state else {
        println!("No recorded Architext serve instance for {}", target.display());
        return Ok(());
    };
    println!("Architext is serving {}", state["target"].as_str().unwrap_or(""));
    println!("ID: {}", state["id"].as_str().unwrap_or(""));
    println!("PID: {}", state["pid"].as_i64().unwrap_or(0));
    println!("Open {}", state["url"].as_str().unwrap_or(""));
    println!("Mode: {}", state["mode"].as_str().unwrap_or("background"));
    if let Some(log) = state["logPath"].as_str() {
        println!("Logs: {log}");
    }
    Ok(())
}

fn stop_state(state: &Value) {
    let pid = state["pid"].as_i64().unwrap_or(0);
    let stopped = stop_serve_process_default(pid);
    if !stopped {
        eprintln!("Architext server {pid} did not stop after SIGTERM and SIGKILL");
    }
    if let Some(id) = state["id"].as_str() {
        remove_serve_state_by_id(id);
    }
}

fn stop_serve(target: &Path, opts: &ParsedArgs) -> Result<(), String> {
    let state = resolve_instance(opts, target)?;
    let Some(state) = state else {
        println!("No recorded Architext serve instance for {}", target.display());
        return Ok(());
    };
    stop_state(&state);
    println!(
        "Stopped Architext serve instance {} for {}",
        state["id"].as_str().unwrap_or(""),
        state["target"].as_str().unwrap_or("")
    );
    Ok(())
}

fn list_serve_instances(opts: &ParsedArgs) -> Result<(), String> {
    let instances = read_serve_instances(true);
    let filtered: Vec<Value> = if !opts.serve_instance.is_empty() {
        instances.iter().filter(|i| i["id"].as_str() == Some(&opts.serve_instance)).cloned().collect()
    } else {
        instances.clone()
    };
    if !opts.serve_instance.is_empty() && filtered.is_empty() {
        return Err(known_instance_error(&opts.serve_instance, &instances));
    }
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&json!({ "instances": filtered })).unwrap());
        return Ok(());
    }
    if filtered.is_empty() {
        println!("No recorded Architext serve instances are running.");
        return Ok(());
    }
    println!("Architext serve instances:");
    for instance in &filtered {
        println!(
            "{}  {}  {}  {}  {}",
            instance["id"].as_str().unwrap_or(""),
            instance["pid"].as_i64().unwrap_or(0),
            instance["mode"].as_str().unwrap_or("background"),
            instance["url"].as_str().unwrap_or(""),
            instance["target"].as_str().unwrap_or("")
        );
        if let Some(log) = instance["logPath"].as_str() {
            println!("  Logs: {log}");
        }
        println!("  Started: {}", instance["startedAt"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn restart_serve(target: &Path, opts: &ParsedArgs, version: &str) -> Result<(), String> {
    let state = resolve_instance(opts, target)?;
    let Some(state) = state else {
        println!("No recorded Architext serve instance for {}", target.display());
        return Ok(());
    };
    if state["mode"].as_str() == Some("foreground") {
        return Err("Foreground serve instances cannot be restarted; stop the owning terminal process and start serve again.".to_string());
    }
    let state_target = PathBuf::from(state["target"].as_str().unwrap_or(""));
    println!("Syncing Architext target before restart: {}", state_target.display());
    // refreshTarget = sync (quiet, non-interactive). Reuse the ported sync.
    refresh_target(&state_target, version);
    stop_state(&state);

    // Re-spawn on the SAME port the old instance held.
    let restart_opts = ParsedArgs {
        host: state["host"].as_str().unwrap_or(&opts.host).to_string(),
        port: state["port"].as_u64().unwrap_or(opts.port as u64) as u32,
        background: true,
        open: false,
        no_open: true,
        serve_restart: false,
        ..opts.clone()
    };
    serve_background(&state_target, &restart_opts)?;
    println!(
        "Restarted Architext background server {} for {}",
        state["id"].as_str().unwrap_or(""),
        state_target.display()
    );
    Ok(())
}

/// `refreshTarget` = the sync used before a restart. Mirrors the JS
/// `syncTarget(refreshTarget, { quiet, branch: none, noAgents, noGitignore,
/// noRootScripts, skipValidate })`.
fn refresh_target(target: &Path, version: &str) {
    let sync_opts = ParsedArgs {
        command: "sync".to_string(),
        target: target.to_string_lossy().to_string(),
        quiet: true,
        branch: "none".to_string(),
        no_agents: true,
        no_gitignore: true,
        no_root_scripts: true,
        skip_validate: true,
        ..default_args()
    };
    crate::commands::sync::run(target, &sync_opts, version);
}

fn default_args() -> ParsedArgs {
    // Build a zeroed ParsedArgs by parsing an empty argv (default command sync).
    crate::args::parse_args(&[]).expect("default args parse")
}

// ─── dispatch ───────────────────────────────────────────────────────────────────

/// Entry point for the `serve` command. Mirrors `runServeLifecycle`.
pub fn run(target: &Path, opts: &ParsedArgs, version: &str) {
    let result: Result<(), String> = if opts.serve_list {
        list_serve_instances(opts)
    } else if opts.serve_status {
        serve_status(target, opts)
    } else if opts.serve_stop {
        stop_serve(target, opts)
    } else if opts.serve_restart {
        restart_serve(target, opts, version)
    } else if opts.background {
        serve_background(target, opts)
    } else {
        serve_foreground(target, opts);
        Ok(())
    };
    if let Err(msg) = result {
        eprintln!("{msg}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_shape() {
        let s = now_iso8601();
        // YYYY-MM-DDTHH:MM:SS.sssZ  (24 chars)
        assert_eq!(s.len(), 24, "got {s}");
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
    }
}
