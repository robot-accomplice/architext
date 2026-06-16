//! Port of `handleBranch` from `architext-cli.mjs`.

use std::path::Path;
use std::process::Command;

/// Check if git is available in `target`.
pub fn git_available(target: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a git command in `target`; returns stdout on success.
fn git(target: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(target)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

/// Port of `handleBranch({ target, options, version, branchChoice })`.
///
/// Returns the branch name if a new branch was created, or None.
pub fn handle_branch(
    target: &Path,
    branch_choice: &str,
    dry_run: bool,
    version: &str,
    branch_name_override: Option<&str>,
) -> Result<Option<String>, String> {
    if dry_run || branch_choice == "none" || !git_available(target) {
        return Ok(None);
    }
    if branch_choice == "current" {
        return Ok(None);
    }
    if branch_choice != "new" {
        return Err("--branch must be current, new, or none".to_string());
    }

    let branch_name = match branch_name_override {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => {
            let v = version.replace('.', "-");
            format!("architext/data-only-{v}")
        }
    };

    git(target, &["checkout", "-b", &branch_name])
        .map_err(|e| format!("git checkout -b {branch_name}: {e}"))?;

    Ok(Some(branch_name))
}
