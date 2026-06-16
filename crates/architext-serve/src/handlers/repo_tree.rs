//! Handler for `GET /api/repo-tree`.
//!
//! Returns the target repository's file list for the Repo Tree viewer.
//! Prefers `git ls-files` (tracked files, honours .gitignore); falls back
//! to a filtered filesystem walk when the target is not a git work tree.
//! Each file is stat'd for `{ path, size, mtime }`.
//!
//! Port of `repoTreeFiles` / `repoTreeApiRequest` in
//! `src/adapters/http/repo-tree-api.mjs`.
//!
//! Cache-Control: no-store (matches JS).

use std::path::Path;
use std::process::Command;

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use serde_json::{json, Value};

use crate::AppState;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Directories to skip in the filesystem walk. Port of `IGNORED_DIRS` in JS.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "dist",
    ".vite",
    "coverage",
    ".nyc_output",
    ".cache",
    ".next",
    ".turbo",
];

/// Maximum number of files returned from the filesystem walk. Port of `MAX_WALK_FILES`.
const MAX_WALK_FILES: usize = 20_000;

// ─── Git helpers ──────────────────────────────────────────────────────────────

/// Returns true if `target` is inside a git work tree.
/// Port of `gitAvailable(target)` in `src/adapters/cli/runtime.mjs`.
fn git_available(target: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `git ls-files` in `target` and return the sorted file paths.
/// Returns `None` if git fails or produces no output.
fn git_ls_files(target: &Path) -> Option<Vec<String>> {
    let out = Command::new("git")
        .args(["ls-files"])
        .current_dir(target)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let tracked: Vec<String> = text
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    if tracked.is_empty() {
        None
    } else {
        Some(tracked)
    }
}

// ─── Filesystem walk ──────────────────────────────────────────────────────────

/// Recursive filesystem walk, stopping at `IGNORED_DIRS` and `MAX_WALK_FILES`.
/// Returns paths relative to `root`, sorted.
///
/// Port of `filesystemWalk(root)` in the JS.
fn filesystem_walk(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk_dir(root, root, &mut files);
    files.sort();
    files
}

fn walk_dir(root: &Path, dir: &Path, files: &mut Vec<String>) {
    if files.len() >= MAX_WALK_FILES {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    // Collect entries so we can detect early-out consistently (matches JS push-then-return).
    let mut entry_list: Vec<std::fs::DirEntry> = entries.flatten().collect();
    entry_list.sort_by_key(|e| e.file_name());

    for entry in entry_list {
        if files.len() >= MAX_WALK_FILES {
            return;
        }
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if ft.is_dir() {
            if IGNORED_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            walk_dir(root, &entry.path(), files);
        } else if ft.is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            // Normalise path separators to forward slash (Windows portability).
            let rel = rel.replace('\\', "/");
            files.push(rel);
        }
    }
}

// ─── Stat entries ─────────────────────────────────────────────────────────────

/// Stat each relative path under `target`.
///
/// Files that cannot be stat'd get `null` size/mtime (matches JS behaviour:
/// "listed by git but absent on disk"). mtime is integer milliseconds
/// (JS `Math.round(info.mtimeMs)`).
///
/// Concurrency: Rust can do this synchronously with a bounded thread pool via
/// rayon, but for correctness and simplicity we stat sequentially on the
/// calling thread (the OS stat call is cheap). The JS STAT_CONCURRENCY=64 bound
/// exists to avoid saturating open-file descriptors; our sequential approach
/// satisfies the same bound trivially (1 fd at a time).
///
/// The output ordering follows the sorted `paths` input exactly — parity with JS.
fn stat_entries(target: &Path, paths: &[String]) -> Vec<Value> {
    paths
        .iter()
        .map(|relative| {
            let abs = target.join(relative);
            match std::fs::metadata(&abs) {
                Ok(meta) => {
                    let size = meta.len() as i64;
                    let mtime = metadata_mtime_ms(&meta);
                    json!({ "path": relative, "size": size, "mtime": mtime })
                }
                Err(_) => json!({ "path": relative, "size": null, "mtime": null }),
            }
        })
        .collect()
}

/// Return the file's mtime as integer milliseconds, byte-matching Node's
/// `Math.round(info.mtimeMs)`. Node derives `mtimeMs = secs*1000 + nsec/1e6`
/// (a float, well within f64 precision since secs*1000 ≈ 1.7e12 ≪ 2^53) and
/// `Math.round`s it. We reconstruct the SAME float and round it with the proven
/// `js_round` (Math.round semantics, half toward +∞) — NOT `as_millis()`, which
/// truncates and diverges by 1ms whenever the sub-ms fraction is ≥ 0.5 (real on
/// nanosecond-precision filesystems like APFS).
fn metadata_mtime_ms(meta: &std::fs::Metadata) -> i64 {
    use std::time::UNIX_EPOCH;
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| {
            let mtime_ms = (d.as_secs() as f64) * 1000.0 + (d.subsec_nanos() as f64) / 1_000_000.0;
            architext_routing::js_compat::js_round(mtime_ms) as i64
        })
        .unwrap_or(0)
}

// ─── Public repo-tree builder ─────────────────────────────────────────────────

/// Build the `/api/repo-tree` response payload.
///
/// Port of `repoTreeFiles(target)`.
/// Returns `{ source: "git"|"filesystem", files: [{ path, size, mtime }, ...] }`.
pub fn build_repo_tree(target: &Path) -> Value {
    let (paths, source) = if git_available(target) {
        match git_ls_files(target) {
            Some(p) => (p, "git"),
            None => (filesystem_walk(target), "filesystem"),
        }
    } else {
        (filesystem_walk(target), "filesystem")
    };

    let files = stat_entries(target, &paths);
    json!({ "source": source, "files": files })
}

// ─── HTTP handler ─────────────────────────────────────────────────────────────

/// GET /api/repo-tree → `{ source, files }` + Cache-Control: no-store
pub async fn get_repo_tree(Extension(state): Extension<AppState>) -> Response {
    let payload = build_repo_tree(&state.target_dir);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ─── ignored-dir filter ───────────────────────────────────────────────────

    #[test]
    fn filesystem_walk_skips_ignored_dirs() {
        let td = temp_dir();
        for &ignored in IGNORED_DIRS {
            let dir = td.path().join(ignored);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("should_be_skipped.txt"), b"x").unwrap();
        }
        fs::write(td.path().join("visible.txt"), b"y").unwrap();
        let files = filesystem_walk(td.path());
        assert_eq!(files, vec!["visible.txt"]);
    }

    #[test]
    fn filesystem_walk_returns_sorted_paths() {
        let td = temp_dir();
        fs::write(td.path().join("z.txt"), b"").unwrap();
        fs::write(td.path().join("a.txt"), b"").unwrap();
        fs::write(td.path().join("m.txt"), b"").unwrap();
        let files = filesystem_walk(td.path());
        assert_eq!(files, vec!["a.txt", "m.txt", "z.txt"]);
    }

    #[test]
    fn filesystem_walk_respects_max_file_cap() {
        let td = temp_dir();
        // Create MAX_WALK_FILES + 10 files
        for i in 0..MAX_WALK_FILES + 10 {
            fs::write(td.path().join(format!("file_{i:06}.txt")), b"").unwrap();
        }
        let files = filesystem_walk(td.path());
        assert!(files.len() <= MAX_WALK_FILES);
    }

    #[test]
    fn filesystem_walk_includes_nested_files() {
        let td = temp_dir();
        fs::create_dir_all(td.path().join("sub").join("deep")).unwrap();
        fs::write(td.path().join("sub").join("deep").join("file.rs"), b"").unwrap();
        fs::write(td.path().join("top.txt"), b"").unwrap();
        let files = filesystem_walk(td.path());
        assert!(files.contains(&"sub/deep/file.rs".to_string()));
        assert!(files.contains(&"top.txt".to_string()));
    }

    // ─── stat entries ─────────────────────────────────────────────────────────

    #[test]
    fn stat_entries_returns_correct_shape() {
        let td = temp_dir();
        let content = b"hello world";
        fs::write(td.path().join("test.txt"), content).unwrap();
        let entries = stat_entries(td.path(), &["test.txt".to_string()]);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e["path"].as_str(), Some("test.txt"));
        assert_eq!(e["size"].as_i64(), Some(content.len() as i64));
        let mtime = e["mtime"].as_i64().unwrap();
        assert!(mtime > 0, "mtime should be a positive integer ms");
    }

    #[test]
    fn stat_entries_missing_file_returns_null() {
        let td = temp_dir();
        let entries = stat_entries(td.path(), &["nonexistent.txt".to_string()]);
        assert_eq!(entries.len(), 1);
        assert!(entries[0]["size"].is_null());
        assert!(entries[0]["mtime"].is_null());
    }

    // ─── build_repo_tree ─────────────────────────────────────────────────────

    #[test]
    fn build_repo_tree_shape() {
        let td = temp_dir();
        fs::write(td.path().join("foo.txt"), b"x").unwrap();
        let result = build_repo_tree(td.path());
        assert!(result.get("source").is_some(), "missing source");
        assert!(result.get("files").is_some(), "missing files");
        let source = result["source"].as_str().unwrap();
        assert!(source == "git" || source == "filesystem", "unexpected source: {source}");
    }

    #[test]
    fn build_repo_tree_filesystem_fallback() {
        // A temp dir with no git repo falls back to filesystem walk.
        let td = temp_dir();
        fs::write(td.path().join("alpha.txt"), b"a").unwrap();
        fs::write(td.path().join("beta.txt"), b"b").unwrap();
        // If we're inside the git repo the source may be "git" — but the files
        // array must always contain the files we created IF we're in filesystem mode.
        let result = build_repo_tree(td.path());
        let source = result["source"].as_str().unwrap();
        if source == "filesystem" {
            let files: Vec<String> = result["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["path"].as_str().unwrap().to_string())
                .collect();
            assert!(files.contains(&"alpha.txt".to_string()));
            assert!(files.contains(&"beta.txt".to_string()));
        }
    }
}
