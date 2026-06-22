//! The Trunk-built viewer dist embedded into the binary (rust-embed).
//!
//! Release builds bake `crates/architext-viewer/dist` into the binary; debug
//! builds read it from disk at runtime. This makes a standalone native install
//! (curl installer / `architext update`) self-contained — `serve` no longer
//! depends on a co-located `<exe_dir>/dist` that the GitHub-release binary lacks.

use std::borrow::Cow;

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../architext-viewer/dist"]
struct ViewerAssets;

/// Embedded asset bytes by relative path (e.g. `"index.html"`, `"styles-….css"`).
/// `None` if the path isn't embedded (or, in dev, isn't on disk).
pub fn embedded_asset(path: &str) -> Option<Cow<'static, [u8]>> {
    ViewerAssets::get(path).map(|f| f.data)
}

/// Whether the embedded viewer is available (true in any release binary built
/// after a `trunk build`; in dev, true when the dist folder is Trunk-built).
pub fn has_embedded_viewer() -> bool {
    ViewerAssets::get("index.html").is_some()
}
