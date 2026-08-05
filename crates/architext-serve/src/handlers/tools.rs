//! External-tool discovery and invocation for the enrichment modes.
//!
//! Code Graph and Slop Detection are the only two modes whose data Architext
//! does not produce itself — they are enrichments from separate tools. That is
//! a real dependency, and the viewer previously said only "no data registered",
//! which tells a reader what is absent and nothing about what to do. This
//! module exists so the empty state can instead say: here is the tool you need,
//! here is whether it is installed, here is where to get it, and — when it IS
//! installed — here is a button that runs it.
//!
//! Shelling out is not new for serve (`node_git.rs` already runs `git`), but
//! these are third-party binaries, so: discovery never fails the request, and
//! every invocation reports the tool's OWN stderr verbatim rather than a
//! paraphrase. A wrapper that swallows the tool's diagnostics is worse than no
//! wrapper — the maintainer has to reproduce by hand to learn anything.

use std::path::{Path, PathBuf};
use std::process::Command;

use axum::extract::Extension;
use axum::http::StatusCode;
use axum::response::Response;
use axum::body::Bytes;
use serde_json::{json, Value};

use crate::AppState;

/// Tools the enrichment modes depend on, with where to get them.
///
/// The repo URL is part of the CONTRACT of this struct, not decoration: an
/// empty state that names a missing binary without saying where it lives just
/// relocates the dead end.
struct Tool {
    /// Binary name as invoked.
    bin: &'static str,
    /// What it produces for us, in the user's terms.
    provides: &'static str,
    repo: &'static str,
}

const MAGMA: Tool = Tool {
    bin: "magma",
    provides: "the call graph Code Graph renders",
    repo: "https://github.com/robot-accomplice/magma",
};

const FERRET: Tool = Tool {
    bin: "ferret",
    provides: "the sweep plan and candidates Slop Detection renders",
    repo: "https://github.com/robot-accomplice/slop-ferret",
};

/// Resolve a binary on PATH. Returns its absolute path when found.
///
/// Deliberately does NOT use `which` (not guaranteed present, and a subprocess
/// to find a subprocess is silly). Walks PATH directly so the answer is the
/// same one `Command::new` will get.
fn find_on_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(bin);
        candidate.is_file().then_some(candidate)
    })
}

/// Best-effort version string. `None` when the tool has no `version` verb or
/// errors — reported as unknown rather than treated as "not installed", since
/// a present-but-unversionable binary is still usable.
fn tool_version(bin: &Path) -> Option<String> {
    let out = Command::new(bin).arg("version").output().ok()?;
    let text = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout });
    // Tools print a whole banner ("magma 0.2.0 - deterministic call-graph ..."),
    // so pull just the version token. The UI shows "v0.2.0"; the banner is noise
    // in a sentence about whether the tool is present.
    let first = text.lines().next()?.trim().to_string();
    let semver = first
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.').to_string());
    semver.or(Some(first)).filter(|v| !v.is_empty())
}

fn describe(tool: &Tool) -> Value {
    match find_on_path(tool.bin) {
        Some(p) => json!({
            "bin": tool.bin,
            "installed": true,
            "path": p.to_string_lossy(),
            "version": tool_version(&p),
            "provides": tool.provides,
            "repo": tool.repo,
        }),
        None => json!({
            "bin": tool.bin,
            "installed": false,
            "path": Value::Null,
            "version": Value::Null,
            "provides": tool.provides,
            "repo": tool.repo,
        }),
    }
}

/// `GET /api/tools` — which enrichment tools are available.
///
/// Read-only and unauthenticated, like `/api/status`: it discloses only whether
/// two well-known binaries exist on the server's PATH, which the operator
/// already knows about their own machine.
pub async fn get_tools() -> Response {
    let body = json!({ "magma": describe(&MAGMA), "ferret": describe(&FERRET) });
    json_response(StatusCode::OK, body)
}

fn json_response(code: StatusCode, body: Value) -> Response {
    let mut resp = Response::new(body.to_string().into());
    *resp.status_mut() = code;
    resp.headers_mut()
        .insert(axum::http::header::CONTENT_TYPE, "application/json".parse().unwrap());
    resp
}

/// Run a command, capturing status and BOTH streams.
fn run(cmd: &mut Command) -> (bool, String, String) {
    match cmd.output() {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ),
        Err(e) => (false, String::new(), format!("failed to spawn: {e}")),
    }
}

/// `POST /api/tools/run` with `{"tool": "magma" | "slop-ferret"}`.
///
/// Runs the enrichment pipeline for the target repo and writes its data file,
/// so the SSE data-events watcher picks it up and the mode populates without a
/// reload.
///
/// Every failure returns the tool's own stderr. These pipelines refuse for
/// GOOD reasons the user must see verbatim — ferret rejects a dirty-tree map
/// with "a dirty map reports in-flight code as dead", which is precisely the
/// sentence that should reach the reader. Paraphrasing it would destroy the
/// only useful part.
pub async fn post_tools_run(Extension(state): Extension<AppState>, body: Bytes) -> Response {
    let payload: Value = serde_json::from_slice(if body.is_empty() { b"{}" } else { &body })
        .unwrap_or_else(|_| json!({}));
    let tool = payload.get("tool").and_then(Value::as_str).unwrap_or("").to_string();
    let target = state.target_dir.clone();

    let result = tokio::task::spawn_blocking(move || match tool.as_str() {
        "magma" => run_magma(&target),
        "slop-ferret" => run_slop_ferret(&target),
        other => json!({ "ok": false, "error": format!("unknown tool \"{other}\"") }),
    })
    .await
    .unwrap_or_else(|e| json!({ "ok": false, "error": format!("task failed: {e}") }));

    let code =
        if result.get("ok").and_then(Value::as_bool) == Some(true) { StatusCode::OK } else { StatusCode::BAD_REQUEST };
    json_response(code, result)
}

/// Build a failure payload whose `error` IS the tool's own words.
///
/// `post_mutation` on the viewer side surfaces `error` and nothing else, so a
/// response carrying only `stderr` reaches the user as "The write was
/// rejected." That happened, and it threw away a perfectly good magma
/// diagnostic. Every failure here goes through this.
fn tool_failure(tool: &str, stderr: &str, hint: Option<&str>) -> Value {
    let trimmed = stderr.trim();
    let error = if trimmed.is_empty() {
        format!("{tool} failed without output.")
    } else {
        trimmed.to_string()
    };
    json!({ "ok": false, "tool": tool, "error": error, "stderr": stderr, "hint": hint })
}

/// magma writes `code-graph.json` into the target repo itself, so all we do is
/// invoke it and report. The vault argument is required by magma but is not our
/// artifact; a temp dir keeps its Obsidian map out of the user's tree.
fn run_magma(target: &Path) -> Value {
    let Some(bin) = find_on_path(MAGMA.bin) else {
        return json!({ "ok": false, "error": format!("`{}` is not installed", MAGMA.bin), "repo": MAGMA.repo });
    };
    let vault = std::env::temp_dir().join("architext-magma-vault");
    let _ = std::fs::create_dir_all(&vault);
    let (ok, stdout, stderr) = run(Command::new(&bin).arg(target).arg("map").arg(&vault));
    if !ok {
        return tool_failure(
            "magma",
            &stderr,
            Some("The message above is verbatim from magma. It writes code-graph.json itself, so this is an upstream failure, not an Architext one."),
        );
    }
    json!({ "ok": true, "tool": "magma", "stdout": stdout, "stderr": stderr })
}

/// The sweep pipeline: magma builds the map ferret reads, ferret derives the
/// plan and an undispositioned discharge, and this binary bundles them.
///
/// The bundle carries the plan's CANDIDATES, which ferret locates and
/// classifies with no model involved — so this run produces real, actionable
/// content on its own. Verifying a candidate into a finding is separate work
/// (the slop-ferret skill), and the panel labels the difference.
fn run_slop_ferret(target: &Path) -> Value {
    let Some(ferret) = find_on_path(FERRET.bin) else {
        return json!({ "ok": false, "error": format!("`{}` is not installed", FERRET.bin), "repo": FERRET.repo });
    };
    let Some(magma) = find_on_path(MAGMA.bin) else {
        return json!({ "ok": false, "error":
            format!("`{}` is not installed, and ferret reads the map it builds", MAGMA.bin), "repo": MAGMA.repo });
    };
    let work = std::env::temp_dir().join("architext-ferret");
    let _ = std::fs::create_dir_all(&work);

    let (ok, _, stderr) = run(Command::new(&magma).arg(target).arg("map").arg(&work));
    if !ok {
        return tool_failure("magma", &stderr, Some("ferret cannot plan without a map of this tree."));
    }
    // magma stamps the SHORT sha; ferret compares the argument literally, so
    // read it back rather than passing our own `git rev-parse` (which is long
    // and would be refused with a misleading "different tree" message).
    let dead = work.join("map/.magma/_dead.json");
    let sha = std::fs::read_to_string(&dead)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("sha").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    if sha.is_empty() {
        return tool_failure("magma", &stderr, Some("magma produced no map sha."));
    }

    let map_dir = work.join("map");
    let (ok, plan, stderr) = run(Command::new(&ferret).arg("plan").arg(&map_dir).arg(&sha).arg(target));
    if !ok {
        return tool_failure("ferret plan", &stderr, None);
    }
    let plan_path = work.join("plan.json");
    if std::fs::write(&plan_path, &plan).is_err() {
        return json!({ "ok": false, "error": "could not write the plan" });
    }
    let (ok, discharge, stderr) = run(Command::new(&ferret).arg("discharge").arg(&plan_path));
    if !ok {
        return tool_failure("ferret discharge", &stderr, None);
    }
    let discharge_path = work.join("discharge.json");
    let findings_path = work.join("findings.json");
    if std::fs::write(&discharge_path, &discharge).is_err()
        || std::fs::write(&findings_path, r#"{"findings":[]}"#).is_err()
    {
        return json!({ "ok": false, "error": "could not write the discharge" });
    }

    // Bundle with THIS binary, so the snapshot always matches the schema this
    // build validates against.
    let Ok(me) = std::env::current_exe() else {
        return json!({ "ok": false, "error": "could not locate the architext binary" });
    };
    let (ok, stdout, stderr) = run(Command::new(me)
        .arg("slop-ferret")
        .arg(target)
        .arg("--plan").arg(&plan_path)
        .arg("--discharge").arg(&discharge_path)
        .arg("--findings").arg(&findings_path));
    if !ok {
        return tool_failure("slop-ferret bundle", &stderr, None);
    }
    json!({ "ok": true, "tool": "slop-ferret", "stdout": stdout, "stderr": stderr })
}
