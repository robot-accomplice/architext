//! Transactional write-set and write-lock infrastructure.
//!
//! Port of the JS patterns from:
//!   `src/adapters/cli/write-lock.mjs` — `withTargetWriteLock` / `waitForDataWritesToSettle`
//!   `src/adapters/http/rules-api.mjs`  — `createWriteSet`
//!   `src/adapters/http/notes-api.mjs`  — `createWriteSet`
//!
//! The Rust serve binary is single-tenant (one data directory), so the
//! write-lock is a per-process `tokio::sync::Mutex` rather than the
//! filesystem lock used by the JS CLI (which must co-ordinate with external
//! processes like the sync daemon).  The snapshot/restore behaviour is
//! preserved byte-for-byte: `capture` reads current bytes, and `restore`
//! writes back exactly those bytes via `std::fs::write` (not `write_json_string`
//! — the file may not exist at restore time, and we want exact byte restoration,
//! not re-serialisation).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

/// A snapshot entry for a single file.
#[derive(Debug, Clone)]
enum Snapshot {
    /// File existed; these are the exact bytes to restore.
    Existed(Vec<u8>),
    /// File did not exist; restore by deleting.
    DidNotExist,
}

/// A write-set captures the prior state of every file written during a
/// mutation transaction so they can be atomically restored on failure.
///
/// This is NOT thread-safe — it is instantiated fresh per request and used
/// within the write-lock guard only.
pub struct WriteSet {
    /// Ordered list of (path, snapshot) so `restore` undoes in reverse order.
    entries: Vec<(PathBuf, Snapshot)>,
    /// Index: path → index in `entries` (for deduplication — only capture once).
    index: HashMap<PathBuf, usize>,
}

impl Default for WriteSet {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteSet {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Capture the current state of `path` before writing.
    ///
    /// Idempotent: if `path` was already captured, does nothing (the first
    /// snapshot is the pre-write state we want to restore to).
    async fn capture(&mut self, path: &Path) {
        if self.index.contains_key(path) {
            return;
        }
        let snap = match tokio::fs::read(path).await {
            Ok(bytes) => Snapshot::Existed(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Snapshot::DidNotExist,
            // Any other read error: treat as not-existing (best effort — the
            // subsequent write will surface the real error).
            Err(_) => Snapshot::DidNotExist,
        };
        let idx = self.entries.len();
        self.entries.push((path.to_path_buf(), snap));
        self.index.insert(path.to_path_buf(), idx);
    }

    /// Capture then write `contents` to `path`.
    ///
    /// Equivalent to JS `writeSet.writeJson(file, value)` but operates on
    /// already-serialised bytes (caller calls `write_json_string` first).
    pub async fn write(&mut self, path: &Path, contents: &str) -> std::io::Result<()> {
        self.capture(path).await;
        // Atomic-ish: write to a temp file beside the target, then rename.
        // This matches the intent of `writeJson` in the JS runtime which does
        // the same via `atomically` (tmp + rename).
        let dir = path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
        })?;
        let tmp = dir.join(format!(".tmp-{}.tmp", uuid_v4_hex()));
        tokio::fs::write(&tmp, contents.as_bytes()).await?;
        tokio::fs::rename(&tmp, path).await?;
        Ok(())
    }

    /// Restore all captured files in reverse-insertion order (mirrors JS:
    /// `for (const [file, snapshot] of [...snapshots.entries()].reverse())`).
    pub async fn restore(&self) {
        for (path, snap) in self.entries.iter().rev() {
            match snap {
                Snapshot::Existed(bytes) => {
                    let _ = tokio::fs::write(path, bytes).await;
                }
                Snapshot::DidNotExist => {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
        }
    }
}

/// Generate a short random hex string for temp-file names.
fn uuid_v4_hex() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().fold(String::with_capacity(16), |mut s, byte| {
        s.push_str(&format!("{byte:02x}"));
        s
    })
}

/// Per-process write-lock for the data directory.
///
/// The JS serve adapter uses a filesystem lock (`withTargetWriteLock`) to
/// co-ordinate with external writers (sync daemon, etc.).  In the Rust binary
/// we are the only writer, so an async mutex is sufficient and avoids the
/// filesystem polling overhead.
pub type WriteLock = Arc<Mutex<()>>;

/// Create a new (unlocked) write-lock.
pub fn new_write_lock() -> WriteLock {
    Arc::new(Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// WriteSet captures existing bytes and restores them exactly.
    #[tokio::test]
    async fn restore_restores_exact_bytes() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.json");
        let original = "original content\n";
        tokio::fs::write(&file, original.as_bytes()).await.unwrap();

        let mut ws = WriteSet::new();
        ws.write(&file, "overwritten content\n").await.unwrap();

        // Verify overwrite happened
        let after = tokio::fs::read_to_string(&file).await.unwrap();
        assert_eq!(after, "overwritten content\n");

        ws.restore().await;

        // Verify original bytes are restored exactly
        let restored = tokio::fs::read_to_string(&file).await.unwrap();
        assert_eq!(restored, original);
    }

    /// WriteSet removes a file on restore if it did not exist before.
    #[tokio::test]
    async fn restore_removes_file_that_did_not_exist() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.json");

        // File does not exist initially
        assert!(!file.exists());

        let mut ws = WriteSet::new();
        ws.write(&file, "{}\n").await.unwrap();

        // Verify file was created
        assert!(file.exists());

        ws.restore().await;

        // Verify file is gone
        assert!(!file.exists());
    }

    /// WriteSet captures only the first state (idempotent).
    #[tokio::test]
    async fn capture_is_idempotent_first_state_wins() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("f.json");
        tokio::fs::write(&file, "v1\n").await.unwrap();

        let mut ws = WriteSet::new();
        ws.write(&file, "v2\n").await.unwrap();
        ws.write(&file, "v3\n").await.unwrap(); // second write: should NOT re-capture

        ws.restore().await;

        let restored = tokio::fs::read_to_string(&file).await.unwrap();
        assert_eq!(restored, "v1\n", "should restore to v1, not v2");
    }

    /// Restore reverses multiple files in reverse order.
    #[tokio::test]
    async fn restore_reverses_multiple_files() {
        let dir = TempDir::new().unwrap();
        let f1 = dir.path().join("a.json");
        let f2 = dir.path().join("b.json");
        tokio::fs::write(&f1, "a-original\n").await.unwrap();
        // f2 does not exist initially

        let mut ws = WriteSet::new();
        ws.write(&f1, "a-new\n").await.unwrap();
        ws.write(&f2, "b-new\n").await.unwrap();

        ws.restore().await;

        assert_eq!(tokio::fs::read_to_string(&f1).await.unwrap(), "a-original\n");
        assert!(!f2.exists());
    }
}
