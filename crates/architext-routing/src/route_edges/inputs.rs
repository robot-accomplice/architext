//! Routing input types shared by the model path and its callers.
//!
//! These were originally defined alongside the legacy candidate engine in
//! `orchestration.rs`. The engine has been removed; the deterministic model
//! (`route_model`, driven from `plan_diagram::apply_model_routes`) is the sole
//! router. These types still describe a plan's relationships and node geometry,
//! so they live here in an engine-free module.

use indexmap::IndexMap;

use crate::model::Rect;

/// Mirrors the JS `input` object passed to the router.
pub struct RouteEdgesInput {
    pub style: String,
    pub relationships: Vec<InputRelationship>,
    pub visible_node_ids: Vec<String>,
    pub node_rects: IndexMap<String, NodeRect>,
    pub lane_index_by_node: IndexMap<String, i64>,
    pub row_index_by_node: IndexMap<String, i64>,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub margin_y: f64,
    pub grid_route_max_points: usize,
    pub grid_route_max_expansions: usize,
    pub score_edge_proximity: bool,
}

/// A node rect with optional `fixedPorts` flag and optional per-side anchor
/// overrides (mirrors JS `rect.sideAnchors`). Decision-diamond nodes carry
/// `sideAnchors` so routing anchors at the diamond tips rather than geometric
/// rect-edge midpoints.
#[derive(Debug, Clone)]
pub struct NodeRect {
    pub rect: Rect,
    pub fixed_ports: bool,
    /// Per-side anchor overrides. Present only for nodes with `sideAnchors` in
    /// the input (e.g. `decision:*` diamond nodes). `None` → geometric midpoint.
    pub side_anchors: Option<crate::route_ports::SideAnchors>,
}

/// Relationship descriptor as consumed by the routing layer.
#[derive(Debug, Clone)]
pub struct InputRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relationship_type: Option<String>,
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
    pub label: Option<String>,
    pub display_index: i64,
}

/// Deterministic planner work counters. The legacy candidate engine populated
/// these (`edgesPlanned` / `cheapCandidateCount` / `gridRouteCalls`); the model
/// router exposes no per-plan work counters, so this is now an empty marker that
/// keeps the `plan_diagram_with_stats` return shape stable for existing callers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorpusPlanStats;
