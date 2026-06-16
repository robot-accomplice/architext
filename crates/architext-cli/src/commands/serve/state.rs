//! Serve lifecycle state file management.
//!
//! Port of the state helpers in `src/adapters/cli/serve-lifecycle.mjs`:
//!   - `serveStateKey` / `serveStatePath` (tmpdir `architext-serve/<key>.json`)
//!   - `readServeState` / `writeServeState` / `removeServeState`
//!   - the `{ target, pid, host, port, url, mode, startedAt, logPath }` shape.
//!
//! State files are written as pretty JSON (2-space) + trailing newline, written
//! atomically via a temp file + rename — byte-identical to JS `writeJson`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// `tmpdir()/architext-serve`
pub fn serve_runtime_dir() -> PathBuf {
    std::env::temp_dir().join("architext-serve")
}

/// Port of `serveStateKey(target)`: sha256(resolve(target)) hex, first 24 chars.
pub fn serve_state_key(target: &Path) -> String {
    let resolved = target.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(resolved.as_bytes());
    let digest = hasher.finalize();
    let hex = digest.iter().map(|b| format!("{b:02x}")).collect::<String>();
    hex[..24].to_string()
}

pub fn serve_state_path(target: &Path) -> PathBuf {
    serve_runtime_dir().join(format!("{}.json", serve_state_key(target)))
}

pub fn serve_state_path_by_id(id: &str) -> PathBuf {
    serve_runtime_dir().join(format!("{id}.json"))
}

pub fn serve_log_path(target: &Path) -> PathBuf {
    serve_runtime_dir().join(format!("{}.log", serve_state_key(target)))
}

fn is_valid_id(id: &str) -> bool {
    id.len() == 24 && id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Port of `readServeState(target)`: returns `None` when missing/corrupt and
/// removes the file in that case (mirrors `isMissingOrCorruptStateError`).
pub fn read_serve_state(target: &Path) -> Option<Value> {
    read_state_at(&serve_state_path(target))
}

pub fn read_serve_state_by_id(id: &str) -> Option<Value> {
    if !is_valid_id(id) {
        return None;
    }
    read_state_at(&serve_state_path_by_id(id))
}

fn read_state_at(path: &Path) -> Option<Value> {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) => Some(v),
            Err(_) => {
                // Corrupt (parse failure) → discard.
                let _ = fs::remove_file(path);
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        // Transient errors (EACCES etc.) must not orphan a live server.
        Err(_) => None,
    }
}

/// Port of `writeServeState`: pretty JSON + trailing newline, atomic rename.
pub fn write_serve_state(target: &Path, state: &Value) -> std::io::Result<()> {
    let dir = serve_runtime_dir();
    fs::create_dir_all(&dir)?;
    let path = serve_state_path(target);
    write_json_atomic(&path, state)
}

/// Mirror of JS `writeJson`: `${JSON.stringify(value, null, 2)}\n` to a temp
/// sibling file then rename.
pub fn write_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = format!("{}\n", serde_json::to_string_pretty(value)?);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

pub fn remove_serve_state(target: &Path) {
    let _ = fs::remove_file(serve_state_path(target));
}

pub fn remove_serve_state_by_id(id: &str) {
    let _ = fs::remove_file(serve_state_path_by_id(id));
}

/// Re-read state and remove only if it still belongs to `pid`/`mode`
/// (port of `removeServeStateIfOwned`).
pub fn remove_serve_state_if_owned(target: &Path, pid: i64, mode: &str) {
    if let Some(state) = read_serve_state(target) {
        if state["pid"].as_i64() == Some(pid) && state["mode"].as_str() == Some(mode) {
            remove_serve_state(target);
        }
    }
}
