//! Ensure the embedded viewer-dist folder exists at compile time so rust-embed's
//! `#[folder = "../architext-viewer/dist"]` never fails to compile when the viewer
//! hasn't been Trunk-built yet (dev / `cargo test` builds embed an empty viewer,
//! which is fine — they don't serve it). Release binaries run `trunk build` first,
//! so the folder holds the real assets to bake in.

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dist = std::path::Path::new(&manifest)
        .parent()
        .expect("crates dir")
        .join("architext-viewer")
        .join("dist");
    let _ = std::fs::create_dir_all(&dist);
    println!("cargo:rerun-if-changed={}", dist.display());
}
