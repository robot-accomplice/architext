//! Public input types, output types, and the `BuildRouteForSides` trait for the mount model.

use indexmap::IndexMap;

use crate::model::Rect;
use crate::route_edges::RouteData;

// ---------------------------------------------------------------------------
// Input types for this module
// ---------------------------------------------------------------------------

/// Richer `input` object routeMountModel functions need beyond the minimal
/// RouteInput used in route_edges.rs.
pub struct MountInput<'a> {
    pub visible_node_ids: &'a [String],
    pub node_rects: &'a IndexMap<String, MountRect>,
    pub lane_index_by_node: &'a IndexMap<String, i64>,
    pub row_index_by_node: &'a IndexMap<String, i64>,
    pub canvas_width: f64,
    pub canvas_height: f64,
}

/// A node rect with the optional `fixedPorts` flag from the JS input.
#[derive(Debug, Clone)]
pub struct MountRect {
    pub rect: Rect,
    /// JS `rect.fixedPorts` — when true the optimiser must not re-home endpoints.
    pub fixed_ports: bool,
    /// Per-side anchor overrides (e.g. diamond tips for `decision:*` nodes).
    /// `None` → geometric midpoint. Mirrors JS `rect.sideAnchors`.
    pub side_anchors: Option<crate::route_ports::SideAnchors>,
}

/// A relationship descriptor as seen by the mount model.
#[derive(Debug, Clone)]
pub struct MountRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relationship_type: String,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
    pub display_index: i64,
    // Fields forwarded to route_intent functions:
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
}

/// Callback interface that replaces the JS `buildRouteForSides(rel, startSide, endSide, routeById)`
/// parameter. The orchestration layer wires this up; the mount model calls it without knowing
/// the implementation. Returns `None` when the requested sides cannot be routed.
pub trait BuildRouteForSides {
    fn build(
        &self,
        rel: &MountRelationship,
        start_side: &str,
        end_side: &str,
        route_by_id: &IndexMap<String, RouteData>,
    ) -> Option<RouteData>;
}

// ---------------------------------------------------------------------------
// Surface descriptor (returned by surfacesOf)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SurfaceInfo {
    pub rect: Rect,
    pub side: String,
    pub positions: Vec<f64>,
}

// ---------------------------------------------------------------------------
// MountCostFactors
// ---------------------------------------------------------------------------

/// Raw factor breakdown, mirrors JS `factors` object in `mountCostFactors`.
#[derive(Debug, Clone, Default)]
pub struct MountCostFactors {
    pub collision: f64,
    pub endpoint_traversal: f64,
    pub repeated_crossing: f64,
    pub self_overlap: f64,
    pub shared_segment: f64,
    pub shared_segment_length: f64,
    pub perimeter_fallback: f64,
    pub crossing: f64,
    pub monotonic_backtrack: f64,
    pub bend: f64,
    pub dogleg: f64,
    pub shallow_jog: f64,
    pub cramped: f64,
    pub intent_mismatch: f64,
    pub length: f64,
    pub over_capacity: f64,
}

// ---------------------------------------------------------------------------
// MountTarget
// ---------------------------------------------------------------------------

/// Descriptor for a movable endpoint (subset of EndpointDescriptor, public).
pub struct MountTarget {
    pub id: String,
    pub endpoint_index: usize, // 0 = first, usize::MAX = last
    pub side: String,
    pub rect: Rect,
}

// ---------------------------------------------------------------------------
// ReliefResult
// ---------------------------------------------------------------------------

pub struct ReliefResult {
    /// Reciprocal pairs Phase 1 relocated onto a shared gutter.
    pub pairs: Vec<[String; 2]>,
    /// Whether relief changed any route at all.
    pub any_moved: bool,
}

// ---------------------------------------------------------------------------
// GutterBridge
// ---------------------------------------------------------------------------

pub struct GutterBridge {
    pub request: RouteData,
    pub ret: RouteData,
}
