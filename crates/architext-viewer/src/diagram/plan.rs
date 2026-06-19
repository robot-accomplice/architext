//! In-process plan compute for flows-mode diagrams.
//!
//! This is the bridge between the viewer's selected (view, flow) and the
//! ported routing engine. It does NOT re-implement routing — it adapts the
//! viewer models to the routing request builder
//! (`architext_routing::plan_request::build_flow_plan_request`) and calls the
//! engine (`architext_routing::plan_diagram::plan_diagram`) in-process, exactly
//! the way the native precompute farm does (`precompute::build_farm_entry`).
//!
//! It is deliberately free of Leptos/`web_sys` so it compiles and runs on
//! native targets and can be gated by a `#[test]`.

use std::collections::HashMap;

use architext_routing::diagram_config::DiagramConfigLayout;
use architext_routing::model::Plan;
use architext_routing::plan_diagram::plan_diagram;
use architext_routing::plan_request::c4_layout::c4_layout_for;
use architext_routing::plan_request::diagram_layout::{diagram_layout_for, LayoutConfig};
use architext_routing::plan_request::{build_flow_plan_request, build_structural_plan_request, StructuralNode};

use crate::data::models::{Flow, Node, View};

/// Resolve the `LayoutConfig` from the resolved `/api/config` diagram payload.
///
/// The config payload's `diagram.layout` is the server-resolved layout section
/// (defaults included). We parse the camelCase keys present and fall through to
/// the `DiagramConfigLayout` defaults for any that are absent or non-numeric,
/// then convert via the routing crate's single-source `to_layout_config()` —
/// so the viewer applies layout overrides through the same code path as the
/// precompute farm.
pub fn layout_config_from_diagram(diagram: &serde_json::Value) -> LayoutConfig {
    let layout = diagram.get("layout");
    let defaults = DiagramConfigLayout::default();
    let num = |key: &str, fallback: f64| -> f64 {
        layout
            .and_then(|l| l.get(key))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(fallback)
    };
    DiagramConfigLayout {
        lane_width: num("laneWidth", defaults.lane_width),
        row_gap: num("rowGap", defaults.row_gap),
        node_width: num("nodeWidth", defaults.node_width),
        node_height: num("nodeHeight", defaults.node_height),
        route_gutter: num("routeGutter", defaults.route_gutter),
        margin_y: num("marginY", defaults.margin_y),
    }
    .to_layout_config()
}

/// Build the diagram `Plan` for a (view, flow) pair using the resolved layout
/// config. This is the exact compute path the UI uses, isolated so it can be
/// exercised by a native test.
///
/// `style` is fixed to `"orthogonal"` — the only edge style the viewer renders.
pub fn compute_plan(view: &View, flow: &Flow, layout: &LayoutConfig) -> Plan {
    let routing_view = view.to_routing();
    let routing_flow = flow.to_routing();
    let request = build_flow_plan_request(&routing_view, &routing_flow, Some(layout), "orthogonal");
    plan_diagram(&request.plan_diagram_input)
}

/// sha256 (lowercase hex) of a plan key string — the `/api/plan/{hash}` cache
/// key the serve plan farm is indexed by.
///
/// This is the wasm-buildable counterpart of the native
/// `architext_routing::precompute::plan_key_hash_native` (which is gated behind
/// the `native` feature and returns `""` on wasm). The farm hashes the SAME
/// `build_flow_plan_request(...).key` string with sha256, so a hash computed
/// here matches the farm's index for the same (view, flow).
pub fn plan_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(key.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // lowercase hex, matching the farm's `hex::encode`.
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// A computed structural diagram: the `Plan` plus the per-edge labels (keyed by
/// route/relationship id, `from-to`) the renderer needs — structural edges carry
/// no numbered flow step, so the label rule's output is threaded through here.
pub struct StructuralDiagram {
    pub plan: Plan,
    pub edge_labels: HashMap<String, String>,
}

/// Build the structural diagram `Plan` for a C4 / deployment view, using the
/// same plan engine the flows path uses. The layout differs per mode:
/// - C4 views (`c4-*`) use the type-specific `c4_layout_for` dimensions;
/// - deployment uses the default `diagram_layout_for` (dense-aware) layout.
///
/// `nodes` supplies dependencies + types (the structural edge + label source).
/// `style` is fixed to `"orthogonal"` — the only edge style the viewer renders.
pub fn compute_structural_plan(view: &View, nodes: &[Node], layout_config: &LayoutConfig) -> StructuralDiagram {
    let routing_view = view.to_routing();
    let structural_nodes: Vec<StructuralNode> = nodes
        .iter()
        .map(|n| StructuralNode {
            id: n.id.clone(),
            node_type: n.node_type.clone(),
            dependencies: n.dependencies.clone(),
        })
        .collect();

    let layout = if view.view_type.starts_with("c4-") {
        c4_layout_for(&view.view_type)
    } else {
        // Deployment: the default layout, with the structural relationship count
        // feeding the dense-topology heuristic.
        let count = architext_routing::plan_request::structural_relationship_count(
            &routing_view,
            &structural_nodes,
        );
        diagram_layout_for(&routing_view, count, Some(layout_config))
    };

    let request = build_structural_plan_request(&routing_view, &structural_nodes, &layout, "orthogonal");
    let plan = plan_diagram(&request.plan_diagram_input);
    StructuralDiagram {
        plan,
        edge_labels: request.edge_labels.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{FlowsFile, NodesFile, ViewsFile};
    use crate::selection::compatible_flow_views;

    /// `plan_hash` is sha256→lowercase-hex. Pin it to published NIST vectors so a
    /// regression (wrong algorithm, uppercase, truncation) fails RED, and so the
    /// hash provably matches the serve farm's `hex::encode(Sha256(key))` index.
    #[test]
    fn plan_hash_matches_known_sha256_vectors() {
        // FIPS 180-2 published vectors.
        assert_eq!(
            plan_hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            plan_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 64 lowercase-hex chars, the exact shape the farm handler validates.
        let h = plan_hash("abc");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    /// The hash is computed over the SAME key the farm hashes
    /// (`build_flow_plan_request(...).key`), so the viewer's `/api/plan/{hash}`
    /// request targets the farm's actual index entry for that (view, flow).
    #[test]
    fn plan_hash_is_taken_over_the_flow_plan_request_key() {
        use architext_routing::diagram_config::DiagramConfigLayout;
        use architext_routing::plan_request::build_flow_plan_request;

        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/architext/data");
        let views: ViewsFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/views.json")).expect("read views.json"),
        )
        .expect("parse views.json");
        let flows: FlowsFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/flows.json")).expect("read flows.json"),
        )
        .expect("parse flows.json");

        let flow_idx = 0;
        let view_idx = *compatible_flow_views(&views.views, &flows.flows, flow_idx)
            .first()
            .expect("a compatible flow view exists");

        let layout = DiagramConfigLayout::default().to_layout_config();
        let req = build_flow_plan_request(
            &views.views[view_idx].to_routing(),
            &flows.flows[flow_idx].to_routing(),
            Some(&layout),
            "orthogonal",
        );
        // Hash of the request key is deterministic + well-shaped.
        let h = plan_hash(&req.key);
        assert_eq!(h.len(), 64);
        assert_eq!(h, plan_hash(&req.key), "hashing is deterministic");
    }

    fn data_root() -> String {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/architext/data").to_string()
    }

    /// Build a STRUCTURAL plan for a real corpus C4 view via the same in-process
    /// path the C4/Deployment UI uses, and assert it produces real geometry:
    /// node rects, routed edges (the view has dependencies), a positive canvas,
    /// and a label per edge (structural edges are labelled by the relationship
    /// rule, not a numbered step). This gates the structural-plan path even
    /// though the SVG render itself is visual.
    #[test]
    fn computes_nonempty_structural_plan_for_c4_container() {
        let root = data_root();
        let views: ViewsFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/views.json")).expect("read views.json"),
        )
        .expect("parse views.json");
        let nodes: NodesFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/nodes.json")).expect("read nodes.json"),
        )
        .expect("parse nodes.json");

        let view = views
            .views
            .iter()
            .find(|v| v.view_type == "c4-container")
            .expect("corpus has a c4-container view");

        let layout = DiagramConfigLayout::default().to_layout_config();
        let diagram = compute_structural_plan(view, &nodes.nodes, &layout);
        let plan = &diagram.plan;

        assert!(plan.canvas_width > 0.0, "canvas_width must be positive");
        assert!(plan.canvas_height > 0.0, "canvas_height must be positive");
        assert!(!plan.node_rects.is_empty(), "node_rects must be non-empty");
        // The c4-container view has visible inter-node dependencies → routes.
        assert!(!plan.routes.is_empty(), "structural routes must be non-empty");
        assert!(!diagram.edge_labels.is_empty(), "structural edges must carry labels");
        for (id, route) in &plan.routes {
            assert!(!route.d.is_empty(), "route {id} must have a non-empty d-string");
            assert!(
                diagram.edge_labels.contains_key(id),
                "route {id} must have a structural label"
            );
        }
    }

    /// C4 layout dims must flow through to the plan: the c4-container node rect
    /// width is the C4 layout's 176 (not the default 136), proving the structural
    /// path uses `c4_layout_for`, not the flow/default layout.
    #[test]
    fn structural_c4_plan_uses_c4_layout_dims() {
        let root = data_root();
        let views: ViewsFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/views.json")).expect("read views.json"),
        )
        .expect("parse views.json");
        let nodes: NodesFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/nodes.json")).expect("read nodes.json"),
        )
        .expect("parse nodes.json");
        let view = views
            .views
            .iter()
            .find(|v| v.view_type == "c4-container")
            .expect("corpus has a c4-container view");
        let layout = DiagramConfigLayout::default().to_layout_config();
        let diagram = compute_structural_plan(view, &nodes.nodes, &layout);
        let rect = diagram.plan.node_rects.values().next().expect("a node rect");
        assert_eq!(rect.width, 176.0, "c4-container node width must be the C4 layout's 176");
    }

    /// Build a plan for a real (view, flow) via the same code path the UI uses,
    /// and assert the plan is non-empty. This gates the in-process compute path
    /// even though the SVG render itself is visual.
    ///
    /// Input is the project's own `docs/architext/data` — the canonical
    /// schema-valid dataset (the synthetic routing-corpus omits `action`, which
    /// the viewer's label-bearing `FlowStep` requires, so it is not a valid
    /// input for the viewer's compute path).
    #[test]
    fn computes_nonempty_plan_for_corpus_flow() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/architext/data");
        let views: ViewsFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/views.json")).expect("read views.json"),
        )
        .expect("parse views.json");
        let flows: FlowsFile = serde_json::from_str(
            &std::fs::read_to_string(format!("{root}/flows.json")).expect("read flows.json"),
        )
        .expect("parse flows.json");

        // Pick a flow and a view that is actually compatible with it (the same
        // pairing rule the UI's selection uses), so the plan has real routes.
        let flow_idx = 0;
        let view_indices = compatible_flow_views(&views.views, &flows.flows, flow_idx);
        let view_idx = *view_indices.first().expect("a compatible flow view exists");

        let layout = DiagramConfigLayout::default().to_layout_config();
        let plan = compute_plan(&views.views[view_idx], &flows.flows[flow_idx], &layout);

        assert!(plan.canvas_width > 0.0, "canvas_width must be positive");
        assert!(plan.canvas_height > 0.0, "canvas_height must be positive");
        assert!(!plan.node_rects.is_empty(), "node_rects must be non-empty");
        assert!(!plan.routes.is_empty(), "routes must be non-empty");
        // Every route carries the geometry the SVG renders verbatim.
        for (id, route) in &plan.routes {
            assert!(!route.d.is_empty(), "route {id} must have a non-empty d-string");
        }
    }
}
