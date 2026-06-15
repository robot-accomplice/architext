//! Faithful port of `viewer/src/routing/routeConstants.js`.
//!
//! Every exported constant is reproduced with the identical literal value from
//! the JS source. These feed geometry calculations; a wrong value changes routes.

// ---------------------------------------------------------------------------
// CANVAS_INSET
// ---------------------------------------------------------------------------
pub struct CanvasInset {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

pub const CANVAS_INSET: CanvasInset = CanvasInset {
    left: 24.0,
    right: 24.0,
    top: 30.0,
    bottom: 24.0,
};

// ---------------------------------------------------------------------------
// ROUTE_COST_WEIGHTS
// ---------------------------------------------------------------------------
pub struct RouteCostWeights {
    pub point_count: f64,
    pub bend: f64,
    pub dogleg: f64,
    pub side_direction: f64,
    pub perimeter_fallback: f64,
    pub corner_perimeter_fallback: f64,
    pub perimeter_length: f64,
    pub corner_perimeter_length: f64,
    pub direct_port_reward: f64,
    pub straight_reward: f64,
    pub spline_reward: f64,
    pub spline_flat_penalty: f64,
    pub boundary_violation: f64,
    pub node_collision: f64,
    pub node_clearance: f64,
    pub monotonic_backtrack: f64,
    pub fixed_preferred_gutter: f64,
}

pub const ROUTE_COST_WEIGHTS: RouteCostWeights = RouteCostWeights {
    point_count: 24.0,
    bend: 420.0,
    dogleg: 14000.0,
    side_direction: 260.0,
    perimeter_fallback: 7000.0,
    corner_perimeter_fallback: 12000.0,
    perimeter_length: 8.0,
    corner_perimeter_length: 10.0,
    direct_port_reward: -2000.0,
    straight_reward: -2200.0,
    spline_reward: -1400.0,
    spline_flat_penalty: 90000.0,
    boundary_violation: 14000.0,
    node_collision: 12000.0,
    node_clearance: 120.0,
    monotonic_backtrack: 18.0,
    fixed_preferred_gutter: 36.0,
};

// ---------------------------------------------------------------------------
// ROUTE_SPACING
// ---------------------------------------------------------------------------
pub struct RouteSpacing {
    pub pair_offset: f64,
    pub index_offset_modulo: f64,
    pub index_offset: f64,
    pub spline_pair_offset: f64,
    pub spline_spread_modulo: f64,
    pub spline_spread: f64,
    pub spline_min_curve: f64,
    pub spline_max_curve: f64,
}

pub const ROUTE_SPACING: RouteSpacing = RouteSpacing {
    pair_offset: 40.0,
    index_offset_modulo: 6.0,
    index_offset: 14.0,
    spline_pair_offset: 8.0,
    spline_spread_modulo: 7.0,
    spline_spread: 10.0,
    spline_min_curve: 36.0,
    spline_max_curve: 180.0,
};

// ---------------------------------------------------------------------------
// SPLINE_CURVE_VARIANTS
// ---------------------------------------------------------------------------
pub struct SplineCurveVariant {
    pub multiplier: f64,
    pub spread: f64,
}

pub const SPLINE_CURVE_VARIANTS: [SplineCurveVariant; 10] = [
    SplineCurveVariant { multiplier: 1.0,    spread:  1.0 },
    SplineCurveVariant { multiplier: -1.0,   spread: -1.0 },
    SplineCurveVariant { multiplier: 0.72,   spread:  0.0 },
    SplineCurveVariant { multiplier: -0.72,  spread:  0.0 },
    SplineCurveVariant { multiplier: 1.36,   spread:  1.0 },
    SplineCurveVariant { multiplier: -1.36,  spread: -1.0 },
    SplineCurveVariant { multiplier: 2.1,    spread:  1.0 },
    SplineCurveVariant { multiplier: -2.1,   spread: -1.0 },
    SplineCurveVariant { multiplier: 0.38,   spread:  0.0 },
    SplineCurveVariant { multiplier: -0.38,  spread:  0.0 },
];

// ---------------------------------------------------------------------------
// MOUNT_COST
// ---------------------------------------------------------------------------
pub struct MountCost {
    pub collision: f64,
    pub endpoint_traversal: f64,
    pub repeated_crossing: f64,
    pub self_overlap: f64,
    pub shared_segment: f64,
    pub shared_segment_length: f64,
    pub dogleg: f64,
    pub shallow_jog: f64,
    pub monotonic_backtrack: f64,
    pub perimeter_fallback: f64,
    pub crossing: f64,
    pub intent_mismatch: f64,
    pub over_capacity: f64,
    pub cramped: f64,
    pub bend: f64,
    pub length: f64,
}

pub const MOUNT_COST: MountCost = MountCost {
    collision:            1_000_000_000.0,
    endpoint_traversal:   1_000_000_000.0,
    repeated_crossing:        5_000_000.0,
    self_overlap:             5_000_000.0,
    shared_segment:             200_000.0,
    shared_segment_length:        1_500.0,
    dogleg:                       6_000.0,
    shallow_jog:                  6_000.0,
    monotonic_backtrack:          6_000.0,
    perimeter_fallback:           4_200.0,
    crossing:                     3_000.0,
    intent_mismatch:              1_500.0,
    over_capacity:                1_000.0,
    cramped:                        120.0,
    bend:                           900.0,
    length:                           6.0,
};

// ---------------------------------------------------------------------------
// Arrowhead / legibility constants
// ---------------------------------------------------------------------------
const ARROWHEAD_MARKER_UNITS: f64 = 4.0;
const FLOW_STROKE_WIDTH: f64 = 2.0;
/// Rendered arrowhead base width in px: marker_units * stroke_width = 8 px.
pub const ARROWHEAD_WIDTH: f64 = ARROWHEAD_MARKER_UNITS * FLOW_STROKE_WIDTH;
/// Fraction of an arrowhead that is the minimum legible gap.
pub const MIN_LEGIBLE_GAP_ARROWHEADS: f64 = 0.5;
/// Minimum gap at which two parallel lines still read as two: 4 px.
pub const MIN_LEGIBLE_GAP: f64 = ARROWHEAD_WIDTH * MIN_LEGIBLE_GAP_ARROWHEADS;

pub const MOUNT_MAX_ITERS: u32 = 8;
pub const RECIPROCAL_PARALLEL_OFFSET: f64 = 12.0;

// ---------------------------------------------------------------------------
// Bridge constants
// ---------------------------------------------------------------------------
pub const BRIDGE_MOUNT_OFFSET: f64 = 9.0;
pub const BRIDGE_GUTTER_CLEARANCE: f64 = 14.0;
pub const BRIDGE_LANE_GAP: f64 = 14.0;
pub const BRIDGE_MAX_LANES: u32 = 8;

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------
use crate::model::{Point, Rect};

/// Port of JS `rectCenter(rect)`.
pub fn rect_center(rect: &Rect) -> Point {
    Point {
        x: rect.x + rect.width / 2.0,
        y: rect.y + rect.height / 2.0,
    }
}

/// Port of JS `dedupeBy(items, keyFn)`: returns items with duplicates (by key)
/// removed, preserving the first occurrence.
pub fn dedupe_by<T, K, F>(items: Vec<T>, key_fn: F) -> Vec<T>
where
    K: std::hash::Hash + Eq,
    F: Fn(&T) -> K,
{
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(key_fn(item)))
        .collect()
}

/// Port of JS `createCandidateCollector(target, seen)`.
///
/// Returns a closure that, when called with `Some(candidate)`, adds the
/// candidate to `target` if its point-key has not been seen before.
/// Passing `None` is a no-op (mirrors the `if (!candidate) return` guard).
///
/// The point-key is `"x,y"` pairs joined by `"|"`, matching JS exactly via
/// `js_number_to_string` so floating-point serialisation is V8-faithful.
///
/// Unlike the JS version (which closes over shared mutable state via a single
/// `seen` Set), this returns a stateful closure that owns its own `seen` set.
/// Callers that need to share a `seen` across multiple collectors should pass
/// the set in via the `with_seen` variant below.
pub fn create_candidate_collector<'a, C>(
    target: &'a mut Vec<C>,
    mut seen: std::collections::HashSet<String>,
    mut point_key: impl FnMut(&C) -> String + 'a,
) -> impl FnMut(Option<C>) + 'a {
    move |candidate: Option<C>| {
        let c = match candidate {
            Some(c) => c,
            None => return,
        };
        let key = point_key(&c);
        if seen.contains(&key) {
            return;
        }
        seen.insert(key);
        target.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Constants match JS literals exactly ---

    #[test]
    fn canvas_inset_values() {
        assert_eq!(CANVAS_INSET.left, 24.0);
        assert_eq!(CANVAS_INSET.right, 24.0);
        assert_eq!(CANVAS_INSET.top, 30.0);
        assert_eq!(CANVAS_INSET.bottom, 24.0);
    }

    #[test]
    fn route_cost_weights_values() {
        assert_eq!(ROUTE_COST_WEIGHTS.point_count, 24.0);
        assert_eq!(ROUTE_COST_WEIGHTS.bend, 420.0);
        assert_eq!(ROUTE_COST_WEIGHTS.dogleg, 14000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.side_direction, 260.0);
        assert_eq!(ROUTE_COST_WEIGHTS.perimeter_fallback, 7000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.corner_perimeter_fallback, 12000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.perimeter_length, 8.0);
        assert_eq!(ROUTE_COST_WEIGHTS.corner_perimeter_length, 10.0);
        assert_eq!(ROUTE_COST_WEIGHTS.direct_port_reward, -2000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.straight_reward, -2200.0);
        assert_eq!(ROUTE_COST_WEIGHTS.spline_reward, -1400.0);
        assert_eq!(ROUTE_COST_WEIGHTS.spline_flat_penalty, 90000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.boundary_violation, 14000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.node_collision, 12000.0);
        assert_eq!(ROUTE_COST_WEIGHTS.node_clearance, 120.0);
        assert_eq!(ROUTE_COST_WEIGHTS.monotonic_backtrack, 18.0);
        assert_eq!(ROUTE_COST_WEIGHTS.fixed_preferred_gutter, 36.0);
    }

    #[test]
    fn route_spacing_values() {
        assert_eq!(ROUTE_SPACING.pair_offset, 40.0);
        assert_eq!(ROUTE_SPACING.index_offset_modulo, 6.0);
        assert_eq!(ROUTE_SPACING.index_offset, 14.0);
        assert_eq!(ROUTE_SPACING.spline_pair_offset, 8.0);
        assert_eq!(ROUTE_SPACING.spline_spread_modulo, 7.0);
        assert_eq!(ROUTE_SPACING.spline_spread, 10.0);
        assert_eq!(ROUTE_SPACING.spline_min_curve, 36.0);
        assert_eq!(ROUTE_SPACING.spline_max_curve, 180.0);
    }

    #[test]
    fn spline_curve_variants_values() {
        assert_eq!(SPLINE_CURVE_VARIANTS.len(), 10);
        assert_eq!(SPLINE_CURVE_VARIANTS[0].multiplier, 1.0);
        assert_eq!(SPLINE_CURVE_VARIANTS[0].spread, 1.0);
        assert_eq!(SPLINE_CURVE_VARIANTS[1].multiplier, -1.0);
        assert_eq!(SPLINE_CURVE_VARIANTS[1].spread, -1.0);
        assert_eq!(SPLINE_CURVE_VARIANTS[2].multiplier, 0.72);
        assert_eq!(SPLINE_CURVE_VARIANTS[6].multiplier, 2.1);
        assert_eq!(SPLINE_CURVE_VARIANTS[9].multiplier, -0.38);
        assert_eq!(SPLINE_CURVE_VARIANTS[9].spread, 0.0);
    }

    #[test]
    fn mount_cost_values() {
        assert_eq!(MOUNT_COST.collision, 1_000_000_000.0);
        assert_eq!(MOUNT_COST.endpoint_traversal, 1_000_000_000.0);
        assert_eq!(MOUNT_COST.repeated_crossing, 5_000_000.0);
        assert_eq!(MOUNT_COST.self_overlap, 5_000_000.0);
        assert_eq!(MOUNT_COST.shared_segment, 200_000.0);
        assert_eq!(MOUNT_COST.shared_segment_length, 1_500.0);
        assert_eq!(MOUNT_COST.dogleg, 6_000.0);
        assert_eq!(MOUNT_COST.shallow_jog, 6_000.0);
        assert_eq!(MOUNT_COST.monotonic_backtrack, 6_000.0);
        assert_eq!(MOUNT_COST.perimeter_fallback, 4_200.0);
        assert_eq!(MOUNT_COST.crossing, 3_000.0);
        assert_eq!(MOUNT_COST.intent_mismatch, 1_500.0);
        assert_eq!(MOUNT_COST.over_capacity, 1_000.0);
        assert_eq!(MOUNT_COST.cramped, 120.0);
        assert_eq!(MOUNT_COST.bend, 900.0);
        assert_eq!(MOUNT_COST.length, 6.0);
    }

    #[test]
    fn arrowhead_and_legibility_constants() {
        assert_eq!(ARROWHEAD_WIDTH, 8.0);
        assert_eq!(MIN_LEGIBLE_GAP_ARROWHEADS, 0.5);
        assert_eq!(MIN_LEGIBLE_GAP, 4.0);
        assert_eq!(MOUNT_MAX_ITERS, 8);
        assert_eq!(RECIPROCAL_PARALLEL_OFFSET, 12.0);
    }

    #[test]
    fn bridge_constants() {
        assert_eq!(BRIDGE_MOUNT_OFFSET, 9.0);
        assert_eq!(BRIDGE_GUTTER_CLEARANCE, 14.0);
        assert_eq!(BRIDGE_LANE_GAP, 14.0);
        assert_eq!(BRIDGE_MAX_LANES, 8);
    }

    #[test]
    fn rect_center_fn() {
        let r = Rect { x: 10.0, y: 20.0, width: 80.0, height: 40.0 };
        let c = rect_center(&r);
        assert_eq!(c.x, 50.0);
        assert_eq!(c.y, 40.0);
    }

    #[test]
    fn dedupe_by_preserves_first_occurrence() {
        let items = vec![1, 2, 3, 2, 1, 4];
        let result = dedupe_by(items, |x| *x);
        assert_eq!(result, vec![1, 2, 3, 4]);
    }

    #[test]
    fn dedupe_by_empty() {
        let items: Vec<i32> = vec![];
        let result = dedupe_by(items, |x| *x);
        assert!(result.is_empty());
    }
}
