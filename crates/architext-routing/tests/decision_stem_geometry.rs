//! End-to-end coverage for the decision-diamond STEM through the REAL production
//! path: `build_flow_plan_request` → `plan_diagram`, exactly as the viewer and
//! the serve plan farm route a flow.
//!
//! WHY this exists separately from `corpus_fitness` / `audit_model`: those gates
//! build participant edges DIRECTLY from the flow and call `route_all_coordinated`
//! — they bypass `build_flow_plan_request`, which is where the stem relationship
//! is synthesised. So the stem geometry is invisible to them. This test exercises
//! the production path and asserts the stem is a legal straight vertical that
//! introduces no §0 forbidden artifact.

use std::path::PathBuf;

use architext_routing::model::{Point, Rect};
use architext_routing::plan_diagram::plan_diagram;
use architext_routing::plan_request::types::{Flow, FlowsFile, View, ViewsFile};
use architext_routing::plan_request::view_selection::flow_compatible_with_view;
use architext_routing::plan_request::{build_flow_plan_request, DECISION_STEM_PREFIX};
use architext_routing::route_model::audit::audit_routes;

/// The FlowForge corpus (readable, has the `fresh-install` decision flow).
fn corpus_json(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/corpus")
        .join(name);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

#[test]
fn fresh_install_decision_diamond_has_a_legal_vertical_stem() {
    let views: Vec<View> = serde_json::from_value::<ViewsFile>(corpus_json("views.json"))
        .expect("views.json")
        .views;
    let flows: Vec<Flow> = serde_json::from_value::<FlowsFile>(corpus_json("flows.json"))
        .expect("flows.json")
        .flows;

    let flow = flows
        .iter()
        .find(|f| f.id == "fresh-install")
        .expect("the fresh-install decision flow");
    let view = views
        .iter()
        .find(|v| flow_compatible_with_view(flow, v))
        .expect("a flow-projection view the fresh-install flow fits");

    // Route exactly as the viewer / serve farm does.
    let req = build_flow_plan_request(view, flow, None, "orthogonal");
    let plan = plan_diagram(&req.plan_diagram_input);

    // 1. The stem route is present (the diamond is anchored, not floating).
    let stem = plan
        .routes
        .iter()
        .find(|(id, _)| id.starts_with(DECISION_STEM_PREFIX))
        .map(|(_, r)| r)
        .expect("a decision-stem route in the routed plan");

    // 2. It is a single straight VERTICAL segment (constant x, non-zero length) —
    //    the property that keeps it off the §0 dogleg/Z gate.
    assert_eq!(stem.points.len(), 2, "stem is one straight segment, got {:?}", stem.points);
    assert!(
        (stem.points[0].x - stem.points[1].x).abs() < 1e-6,
        "stem is vertical (constant x): {:?}",
        stem.points
    );
    assert!(
        (stem.points[0].y - stem.points[1].y).abs() > 1.0,
        "stem has real length: {:?}",
        stem.points
    );

    // 3. The full plan — stem included — passes the forbidden-artifact gate, so
    //    the synthesised stem introduces no dogleg / Z / staircase / min-stem.
    let routes: Vec<Vec<Point>> = plan.routes.values().map(|r| r.points.clone()).collect();
    let rects: Vec<Rect> = plan.node_rects.values().cloned().collect();
    let audit = audit_routes(&routes, &rects);
    assert!(
        audit.is_clean(),
        "stem must not introduce a forbidden artifact; forbidden count = {}",
        audit.forbidden()
    );
}
