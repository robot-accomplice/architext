//! Port of `upsertGitignore({ target, dryRun })` from `architext-cli.mjs`.

use std::path::Path;

use super::target_layout::GENERATED_IGNORES;

/// Port of `upsertGitignore`.
/// Returns `(changed, missing_entries)`.
pub fn upsert_gitignore(target: &Path, dry_run: bool) -> std::io::Result<(bool, Vec<&'static str>)> {
    let destination = target.join(".gitignore");
    let existing = if destination.exists() {
        std::fs::read_to_string(&destination)?
    } else {
        String::new()
    };

    // JS: existing.split(/\r?\n/)
    let lines: Vec<&str> = existing.split(['\n', '\r']).filter(|l| !l.is_empty()).collect();
    let missing: Vec<&'static str> = GENERATED_IGNORES
        .iter()
        .copied()
        .filter(|entry| !lines.contains(entry))
        .collect();

    if missing.is_empty() {
        return Ok((false, vec![]));
    }

    if !dry_run {
        let prefix = existing.trim_end();
        let content = if prefix.is_empty() {
            format!(
                "# Architext generated static builds.\n{}\n",
                missing.join("\n")
            )
        } else {
            format!(
                "{prefix}\n\n# Architext generated static builds.\n{}\n",
                missing.join("\n")
            )
        };
        std::fs::write(&destination, content.as_bytes())?;
    }

    Ok((true, missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn adds_missing_entries() {
        let dir = TempDir::new().unwrap();
        let (changed, missing) = upsert_gitignore(dir.path(), false).unwrap();
        assert!(changed);
        assert_eq!(missing.len(), GENERATED_IGNORES.len());
        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        for entry in GENERATED_IGNORES {
            assert!(content.contains(entry), "missing: {entry}");
        }
    }

    #[test]
    fn idempotent() {
        let dir = TempDir::new().unwrap();
        upsert_gitignore(dir.path(), false).unwrap();
        let (changed, _) = upsert_gitignore(dir.path(), false).unwrap();
        assert!(!changed);
    }

    #[test]
    fn dry_run_no_write() {
        let dir = TempDir::new().unwrap();
        upsert_gitignore(dir.path(), true).unwrap();
        assert!(!dir.path().join(".gitignore").exists());
    }
}
