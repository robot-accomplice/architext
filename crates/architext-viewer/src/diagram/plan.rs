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

use architext_routing::diagram_config::DiagramConfigLayout;
use architext_routing::model::Plan;
use architext_routing::plan_diagram::plan_diagram;
use architext_routing::plan_request::build_flow_plan_request;
use architext_routing::plan_request::diagram_layout::LayoutConfig;

use crate::data::models::{Flow, View};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{FlowsFile, ViewsFile};
    use crate::selection::compatible_flow_views;

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
