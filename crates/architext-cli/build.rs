//! Build script: stamp the product version into the binary.
//!
//! The single source of truth for the *product* version is the repo-root
//! `VERSION` file (bumped by the release process). The workspace crates stay at
//! `0.0.0`, so we read `VERSION` at build time and expose its contents as
//! `ARCHITEXT_VERSION` for `main.rs` to report — no second bump site to drift,
//! and no npm/`package.json` dependency (distribution is native-only since
//! 1.7.6). Falls back to the crate's `CARGO_PKG_VERSION` if `VERSION` is absent
//! (e.g. building the crate in isolation).
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let version_file = Path::new(&manifest_dir).join("..").join("..").join("VERSION");
    println!("cargo:rerun-if-changed={}", version_file.display());

    let version = std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());

    println!("cargo:rustc-env=ARCHITEXT_VERSION={version}");
}
