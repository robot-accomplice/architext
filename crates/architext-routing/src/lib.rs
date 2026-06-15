//! Architext routing engine — single source of truth, compiled native (serve)
//! and to WASM (browser). See docs/superpowers/specs/2026-06-15-rust-backend-rewrite-design.md.

pub mod js_compat;
pub mod model;
pub mod wasm;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds_and_links() {
        assert_eq!(2 + 2, 4);
    }
}
