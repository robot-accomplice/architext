//! `clean [path] [--node-modules] [--dry-run]` — port of `cleanGenerated` in
//! `src/adapters/cli/architext-cli.mjs` (~line 1038).

use std::path::Path;
use std::process;

pub fn run(target: &Path, node_modules: bool, dry_run: bool) {
    let arch_dir = target.join("docs").join("architext");
    let mut candidates = vec![arch_dir.join("dist")];
    if node_modules {
        candidates.push(arch_dir.join("node_modules"));
    }

    let mut removed: Vec<std::path::PathBuf> = Vec::new();
    for candidate in &candidates {
        if candidate.exists() {
            removed.push(candidate.clone());
            if !dry_run {
                if let Err(e) = std::fs::remove_dir_all(candidate) {
                    eprintln!("Failed to remove {}: {e}", candidate.display());
                    process::exit(1);
                }
            }
        }
    }

    if removed.is_empty() {
        println!("No generated Architext artifacts found.");
    } else {
        // JS: `${options.dryRun ? "Would remove" : "Removed"}:\n${removed.map(item=>`- ${item}`).join("\n")}`
        let verb = if dry_run { "Would remove" } else { "Removed" };
        let items: Vec<String> = removed
            .iter()
            .map(|p| format!("- {}", p.display()))
            .collect();
        println!("{verb}:\n{}", items.join("\n"));
    }
}
