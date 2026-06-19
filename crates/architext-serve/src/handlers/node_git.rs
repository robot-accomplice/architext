//! Handler for `GET /api/node-git?paths=<comma-separated repo-relative paths>`.
//!
//! Returns the git "development window" for a node's source paths: the first and
//! last commit that touched any of them, the commit count, and the contributing
//! authors. The inspector renders this as "first seen · last changed · active
//! span · N commits · authors" — real timestamps + development time scraped from
//! git, since the Architext schema records none.
//!
//! Paths that git does not track (or a non-git target) return
//! `{ "tracked": false }` so the viewer degrades gracefully. The sanitized review
//! corpus, whose `sourcePaths` do not exist in the serving repo, lands here by
//! design; a real-source project resolves to a populated window.
//!
//! Cache-Control: no-store (git history is live state).

use std::path::Path;
use std::process::Command;

use axum::{
    extract::Query,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct NodeGitQuery {
    /// Comma-separated, repo-relative source paths (a node's `sourcePaths`).
    #[serde(default)]
    pub paths: String,
}

pub async fn get_node_git(
    Extension(state): Extension<AppState>,
    Query(query): Query<NodeGitQuery>,
) -> Response {
    let paths: Vec<String> = query
        .paths
        .split(',')
        .map(str::trim)
        .filter(|p| is_safe_pathspec(p))
        .map(str::to_string)
        .collect();

    let payload = node_git_meta(&state.target_dir, &paths);

    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    let body = serde_json::to_string_pretty(&payload)
        .map(|s| format!("{s}\n"))
        .unwrap_or_else(|_| "{}\n".to_string());
    (StatusCode::OK, headers, body).into_response()
}

/// A safe git pathspec is non-empty, relative, not an option (`-…`), and free of
/// `..` traversal — so a client's `sourcePaths` cannot escape the target repo or
/// smuggle a git flag (everything is also passed after `--`).
fn is_safe_pathspec(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.starts_with('-')
        && !p.split('/').any(|c| c == "..")
}

fn git_available(target: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// True when git tracks at least one file under `paths` in `target`. We only
/// need a yes/no: if nothing is tracked we report `{tracked:false}` rather than
/// a window for paths that don't exist in this repo (the corpus case).
fn any_tracked(target: &Path, paths: &[String]) -> bool {
    Command::new("git")
        .args(["ls-files", "--"])
        .args(paths)
        .current_dir(target)
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn node_git_meta(target: &Path, paths: &[String]) -> Value {
    if paths.is_empty() || !git_available(target) || !any_tracked(target, paths) {
        return json!({ "tracked": false });
    }
    let out = Command::new("git")
        .args(["log", "--format=%aI%x09%an", "--"])
        .args(paths)
        .current_dir(target)
        .output();
    match out {
        Ok(o) if o.status.success() => parse_git_log(&String::from_utf8_lossy(&o.stdout)),
        _ => json!({ "tracked": false }),
    }
}

/// Parse `git log --format=%aI\t%an` output (newest commit first) into the
/// development-window payload. Pure → unit-tested without a live repo.
fn parse_git_log(text: &str) -> Value {
    let rows: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(d, a)| (d.trim(), a.trim()))
        .filter(|(d, _)| !d.is_empty())
        .collect();
    if rows.is_empty() {
        return json!({ "tracked": false });
    }
    // git log lists newest commit first.
    let last_commit = rows.first().unwrap().0;
    let first_commit = rows.last().unwrap().0;
    // Distinct authors, newest-seen first.
    let mut authors: Vec<String> = Vec::new();
    for (_, a) in &rows {
        if !a.is_empty() && !authors.iter().any(|x| x == a) {
            authors.push((*a).to_string());
        }
    }
    json!({
        "tracked": true,
        "firstCommit": first_commit,
        "lastCommit": last_commit,
        "commitCount": rows.len(),
        "authors": authors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_window_from_newest_first_log() {
        let log = "2026-06-18T10:00:00-07:00\tAlice\n\
                   2026-03-01T09:00:00-07:00\tBob\n\
                   2026-01-15T08:00:00-07:00\tAlice\n";
        let v = parse_git_log(log);
        assert_eq!(v["tracked"], true);
        assert_eq!(v["lastCommit"], "2026-06-18T10:00:00-07:00");
        assert_eq!(v["firstCommit"], "2026-01-15T08:00:00-07:00");
        assert_eq!(v["commitCount"], 3);
        // De-duplicated, newest-first.
        assert_eq!(v["authors"], json!(["Alice", "Bob"]));
    }

    #[test]
    fn empty_log_is_untracked() {
        assert_eq!(parse_git_log(""), json!({ "tracked": false }));
        assert_eq!(parse_git_log("\n  \n"), json!({ "tracked": false }));
    }

    #[test]
    fn pathspec_safety() {
        assert!(is_safe_pathspec("crates/architext-serve/src/lib.rs"));
        assert!(is_safe_pathspec("viewer/"));
        assert!(!is_safe_pathspec("../etc/passwd"));
        assert!(!is_safe_pathspec("a/../../b"));
        assert!(!is_safe_pathspec("/abs/path"));
        assert!(!is_safe_pathspec("--output=x"));
        assert!(!is_safe_pathspec(""));
    }
}
