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
pub mod route_candidate_ports;
pub mod route_cache;
pub mod route_index;
pub mod route_scoring;
pub mod route_candidate_builders;
pub mod route_strategies;
pub mod route_style;
pub mod route_edges;
pub mod route_mount_model;
pub mod route_diagnostics;
pub mod route_model;
pub mod plan_diagram;
pub mod wasm;
pub mod plan_request;
pub mod diagram_config;
#[cfg(not(target_arch = "wasm32"))]
pub mod precompute;

/// Whether the deterministic routing model is the routing path for `plan()`
/// diagrams (flows, C4, deployment). It is the DEFAULT — the model carries a
/// permanent forbidden-artifact gate (zero doglegs, Z/staircases, non-orthogonal
/// segments), so doublebacks and step formations cannot occur. Sequence diagrams
/// don't use `plan()`, so they are unaffected regardless.
///
/// Set `ARCHITEXT_ROUTING_ENGINE=1` to fall back to the legacy candidate engine
/// (kept as an escape hatch for comparison / regression triage).
pub fn routing_model_enabled() -> bool {
    std::env::var("ARCHITEXT_ROUTING_ENGINE").is_err()
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds_and_links() {
        assert_eq!(2 + 2, 4);
    }
}
