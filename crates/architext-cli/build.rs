//! Build script: stamp the product version into the binary.
//!
//! The single source of truth for the *product* version is the repo's
//! `package.json` (bumped by the release process). The workspace crates stay at
//! `0.0.0`, so we read `package.json` at build time and expose its version as
//! `ARCHITEXT_VERSION` for `main.rs` to report — no second bump site to drift.
//! Falls back to the crate's `CARGO_PKG_VERSION` if `package.json` is absent
//! (e.g. building the crate in isolation).
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let pkg_json = Path::new(&manifest_dir).join("..").join("..").join("package.json");
    println!("cargo:rerun-if-changed={}", pkg_json.display());

    let version = std::fs::read_to_string(&pkg_json)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(str::to_owned))
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());

    println!("cargo:rustc-env=ARCHITEXT_VERSION={version}");
}
