//! Shared data types used across all route_edges submodules.
//!
//! `RouteData` — the mutable route object threaded through every helper.
//! `RouteInput` / `Relationship` — minimal views of the router input required
//! by collision / traversal helpers.

use indexmap::IndexMap;

use crate::model::{Point, Rect};

// ---------------------------------------------------------------------------
// RouteData — the mutable route object passed between helpers
// ---------------------------------------------------------------------------

/// Represents the JS route object fields read and written by this helper
/// surface. The `controls` field mirrors JS `route.controls?: [Point, Point]`.
/// Extra fields (d, samples, sampleBounds, bends, labelX, labelY) are
/// recomputed by `route_with_points`; `extra` carries opaque JSON fields.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteData {
    /// SVG path string. Rebuilt by `route_with_points`.
    pub d: String,
    /// The simplified orthogonal/spline points list.
    pub points: Vec<Point>,
    /// Optional spline control points (exactly 2 for a cubic bezier).
    pub controls: Option<[Point; 2]>,
    /// Sampled points used for collision/proximity testing.
    pub samples: Vec<Point>,
    /// Bounding box of samples + points.
    pub sample_bounds: Rect,
    /// Number of bends in `points`.
    pub bends: usize,
    /// Label position X.
    pub label_x: f64,
    /// Label position Y.
    pub label_y: f64,
    /// Route style: "orthogonal" | "spline" | "straight".
    pub style: String,
    /// Extra opaque fields (pass-through).
    pub extra: IndexMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Input abstraction for collision checks
// ---------------------------------------------------------------------------

/// Minimal view of the router `input` object required by the ported helpers.
pub struct RouteInput<'a> {
    pub visible_node_ids: &'a [String],
    pub node_rects: &'a IndexMap<String, Rect>,
}

/// Minimal view of a `relationship` object.
pub struct Relationship<'a> {
    pub from: &'a str,
    pub to: &'a str,
}

// ---------------------------------------------------------------------------
// AxisAlignedSegment — shared by helpers + separation
// ---------------------------------------------------------------------------

/// An axis-aligned segment extracted from a route's points list.
///
/// Mirrors the JS `{ orientation, line, min, max }` object returned by
/// `axisAlignedSegments` and `renderedAxisAlignedSegments`.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisAlignedSegment {
    /// "horizontal" or "vertical"
    pub orientation: &'static str,
    /// The constant coordinate (y for horizontal, x for vertical).
    pub line: f64,
    /// Smaller of the two variable-axis coordinates.
    pub min: f64,
    /// Larger of the two variable-axis coordinates.
    pub max: f64,
}
