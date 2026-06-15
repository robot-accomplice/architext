//! Architext routing engine — single source of truth, compiled native (serve)
//! and to WASM (browser). See docs/superpowers/specs/2026-06-15-rust-backend-rewrite-design.md.

pub mod js_compat;
pub mod model;
pub mod priority_queue;
pub mod route_constants;
pub mod route_geometry;
pub mod route_corridors;
pub mod route_intent;
pub mod route_labels;
pub mod route_ports;
pub mod route_reciprocal;
pub mod route_rendering;
pub mod route_scoring;
pub mod route_style;
pub mod wasm;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds_and_links() {
        assert_eq!(2 + 2, 4);
    }
}
