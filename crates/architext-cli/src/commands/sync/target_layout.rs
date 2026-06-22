//! Mirrors `src/domain/lifecycle/target-layout.mjs` constants and path helpers.
//! Kept here (not in architext-core) because only the CLI sync command needs them.

use std::path::{Path, PathBuf};

pub const METADATA_FILE: &str = ".architext.json";
pub const LEGACY_METADATA_FILE: &str = ".architext-install.json";
pub const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
pub const GENERATED_IGNORES: &[&str] = &[
    "docs/architext/dist/",
    "docs/architext/.architext-write.lock/",
];
pub const COPIED_INSTALL_ENTRIES: &[&str] = &[
    "AGENTS_APPENDIX.md",
    "LLM_ARCHITEXT.md",
    "README.md",
    "index.html",
    "dist",
    "node_modules",
    "package-lock.json",
    "package.json",
    "public",
    "schema",
    "src",
    "tools",
    "tsconfig.json",
    "vite.config.ts",
];

pub fn architext_dir(target: &Path) -> PathBuf {
    target.join("docs").join("architext")
}

pub fn data_dir(target: &Path) -> PathBuf {
    architext_dir(target).join("data")
}

pub fn metadata_path(target: &Path) -> PathBuf {
    architext_dir(target).join(METADATA_FILE)
}

pub fn legacy_metadata_path(target: &Path) -> PathBuf {
    architext_dir(target).join(LEGACY_METADATA_FILE)
}

pub fn copied_install_candidate_paths(target: &Path) -> Vec<PathBuf> {
    COPIED_INSTALL_ENTRIES
        .iter()
        .map(|entry| architext_dir(target).join(entry))
        .collect()
}
