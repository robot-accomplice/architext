//! Crossing-count geometry helpers (Pass C4).
//!
//! Ported from `viewer/src/routing/routeEdges.js`:
//! - `crossingsBetween`  (L1572) — inclusive-bound segment-intersection count,
//!   shared mounts excluded. Also exported for routeDiagnostics.
//! - `crossingsInvolving` (L1698) — sum of crossingsBetween for one route vs all others.
//! - `gutterLaneOf`      (L1684) — perpendicular coordinate of the longest
//!   axis-aligned gutter run in a route.

use indexmap::IndexMap;

use super::helpers::axis_aligned_segments;
use super::types::RouteData;

// ---------------------------------------------------------------------------
// crossingsBetween (L1572) — PUBLIC (routeDiagnostics imports it)
// ---------------------------------------------------------------------------

/// Port of JS `crossingsBetween(routeA, routeB)`.
///
/// Counts how many distinct geometry points two orthogonal routes intersect.
/// Uses **inclusive** bounds so T-junctions (corner landing on another edge)
/// count as crossings.  Shared mounts — both routes terminating at the same
/// point — are excluded (two edges meeting at one node convergence are fine).
pub fn crossings_between(route_a: &RouteData, route_b: &RouteData) -> usize {
    if route_a.points.is_empty() || route_b.points.is_empty() {
        return 0;
    }
    let segments_a = axis_aligned_segments(route_a);
    let segments_b = axis_aligned_segments(route_b);

    // Build terminal-point sets as "x,y" keys — same format as the JS Set.
    let pa0 = &route_a.points[0];
    let pa_last = route_a.points.last().unwrap();
    let pb0 = &route_b.points[0];
    let pb_last = route_b.points.last().unwrap();
    let terminal_a: std::collections::HashSet<String> = [
        format!("{},{}", pa0.x, pa0.y),
        format!("{},{}", pa_last.x, pa_last.y),
    ]
    .into();
    let terminal_b: std::collections::HashSet<String> = [
        format!("{},{}", pb0.x, pb0.y),
        format!("{},{}", pb_last.x, pb_last.y),
    ]
    .into();

    let mut points: std::collections::HashSet<String> = std::collections::HashSet::new();

    for left in &segments_a {
        for right in &segments_b {
            if left.orientation == right.orientation {
                continue;
            }
            // Assign horizontal/vertical deterministically.
            let (horizontal, vertical) = if left.orientation == "horizontal" {
                (left, right)
            } else {
                (right, left)
            };
            // vertical.line is an x-coord; horizontal.line is a y-coord.
            if vertical.line >= horizontal.min
                && vertical.line <= horizontal.max
                && horizontal.line >= vertical.min
                && horizontal.line <= vertical.max
            {
                let key = format!("{},{}", vertical.line, horizontal.line);
                // Shared mount: both routes terminate at this exact point.
                if terminal_a.contains(&key) && terminal_b.contains(&key) {
                    continue;
                }
                points.insert(key);
            }
        }
    }

    points.len()
}

// ---------------------------------------------------------------------------
// crossingsInvolving (L1698)
// ---------------------------------------------------------------------------

/// Port of JS `crossingsInvolving(routeById, relationshipId)`.
///
/// Returns the total crossings between `relationship_id`'s route and every
/// other route in the map (each crossing counted once per pair here — the JS
/// iterates all pairs from the perspective of `relationshipId` and sums
/// `crossingsBetween` for each, so a crossing IS counted for every route that
/// touches it; this matches the JS literal behaviour).
pub fn crossings_involving(
    route_by_id: &IndexMap<String, RouteData>,
    relationship_id: &str,
) -> usize {
    let route = match route_by_id.get(relationship_id) {
        Some(r) => r,
        None => return 0,
    };
    let mut total = 0usize;
    for (other_id, other) in route_by_id {
        if other_id == relationship_id {
            continue;
        }
        total += crossings_between(route, other);
    }
    total
}

// ---------------------------------------------------------------------------
// gutterLaneOf (L1684)
// ---------------------------------------------------------------------------

/// Port of JS `gutterLaneOf(route, perpAxis, alongAxis)`.
///
/// Finds the perpendicular-axis coordinate of the longest axis-aligned gutter
/// run in `route`.  Returns `None` when no run exists (route has < 2 points
/// or no segment is constant along `perp_axis`).
///
/// `perp_axis` is `"x"` or `"y"` — the axis held constant in the candidate
/// gutter segments.  `along_axis` is the orthogonal one whose distance is
/// measured.  JS returns `null` when none found; we return `Option<f64>`.
pub fn gutter_lane_of(route: &RouteData, perp_axis: &str, along_axis: &str) -> Option<f64> {
    let mut best_len: f64 = -1.0;
    let mut lane: Option<f64> = None;
    for i in 0..route.points.len().saturating_sub(1) {
        let a = &route.points[i];
        let b = &route.points[i + 1];
        let a_perp = if perp_axis == "x" { a.x } else { a.y };
        let b_perp = if perp_axis == "x" { b.x } else { b.y };
        // Only consider segments constant along perp_axis.
        if a_perp == b_perp {
            let a_along = if along_axis == "x" { a.x } else { a.y };
            let b_along = if along_axis == "x" { b.x } else { b.y };
            let length = (b_along - a_along).abs();
            if length > best_len {
                best_len = length;
                lane = Some(a_perp);
            }
        }
    }
    lane
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Point;

    fn route(points: Vec<(f64, f64)>) -> RouteData {
        use crate::model::Rect;
        let pts: Vec<Point> = points.iter().map(|&(x, y)| Point { x, y }).collect();
        RouteData {
            d: String::new(),
            points: pts,
            controls: None,
            samples: vec![],
            sample_bounds: Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
            bends: 0,
            label_x: 0.0,
            label_y: 0.0,
            style: "orthogonal".into(),
            extra: indexmap::IndexMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // crossingsBetween
    // -----------------------------------------------------------------------

    #[test]
    fn crossings_between_x_cross() {
        // Node: H=(0,50)→(100,50) × V=(50,0)→(50,100) → 1
        let h = route(vec![(0.0, 50.0), (100.0, 50.0)]);
        let v = route(vec![(50.0, 0.0), (50.0, 100.0)]);
        assert_eq!(crossings_between(&h, &v), 1);
    }

    #[test]
    fn crossings_between_t_junction() {
        // Node: T-junction counts (inclusive bounds): H=(0,50)→(100,50), VT=(50,0)→(50,50) → 1
        let h = route(vec![(0.0, 50.0), (100.0, 50.0)]);
        let vt = route(vec![(50.0, 0.0), (50.0, 50.0)]);
        assert_eq!(crossings_between(&h, &vt), 1);
    }

    #[test]
    fn crossings_between_shared_mount_excluded() {
        // Node: RA ends at (0,0), RB ends at (0,0) — shared mount → 0
        // RA=(0,0)→(50,0), RB=(0,50)→(0,0). The segments cross at (0,0).
        // Both routes terminate at (0,0) → excluded.
        let ra = route(vec![(0.0, 0.0), (50.0, 0.0)]);
        let rb = route(vec![(0.0, 50.0), (0.0, 0.0)]);
        assert_eq!(crossings_between(&ra, &rb), 0);
    }

    #[test]
    fn crossings_between_parallel_no_crossing() {
        // Node: two horizontal routes at different y → 0
        let h1 = route(vec![(0.0, 0.0), (100.0, 0.0)]);
        let h2 = route(vec![(0.0, 50.0), (100.0, 50.0)]);
        assert_eq!(crossings_between(&h1, &h2), 0);
    }

    #[test]
    fn crossings_between_empty_route() {
        let empty = route(vec![]);
        let h = route(vec![(0.0, 50.0), (100.0, 50.0)]);
        assert_eq!(crossings_between(&empty, &h), 0);
        assert_eq!(crossings_between(&h, &empty), 0);
    }

    #[test]
    fn crossings_between_l_shape_cross() {
        // Node: L-shape A=(0,0)→(100,0)→(100,100) × horizontal B=(0,50)→(100,50)
        // The vertical segment (100,0)→(100,100) doesn't cross B's x=0..100 at x=100
        // Wait — B goes from x=0 to x=100 inclusive, and V.line=100 is in [0,100].
        // B.line(y=50) is in V.min(0)..V.max(100). Both not shared mounts → counts as 1.
        // Also: H segment (0,0)→(100,0) vs B.vertical: B has no vertical. Only A has V.
        let a = route(vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]);
        let b = route(vec![(0.0, 50.0), (100.0, 50.0)]);
        assert_eq!(crossings_between(&a, &b), 1);
    }

    // -----------------------------------------------------------------------
    // crossingsInvolving
    // -----------------------------------------------------------------------

    #[test]
    fn crossings_involving_single_crossing() {
        // Node: H crosses V; crossingsInvolving for H = 1, for V = 1
        let mut rbd = IndexMap::new();
        rbd.insert("h".to_string(), route(vec![(0.0, 50.0), (100.0, 50.0)]));
        rbd.insert("v".to_string(), route(vec![(50.0, 0.0), (50.0, 100.0)]));
        rbd.insert("h2".to_string(), route(vec![(0.0, 200.0), (100.0, 200.0)]));
        assert_eq!(crossings_involving(&rbd, "h"), 1);
        assert_eq!(crossings_involving(&rbd, "v"), 1);
        assert_eq!(crossings_involving(&rbd, "h2"), 0);
    }

    #[test]
    fn crossings_involving_missing_id() {
        let rbd: IndexMap<String, RouteData> = IndexMap::new();
        assert_eq!(crossings_involving(&rbd, "missing"), 0);
    }

    // -----------------------------------------------------------------------
    // gutterLaneOf
    // -----------------------------------------------------------------------

    #[test]
    fn gutter_lane_of_l_shape_perp_x() {
        // Node: L=(0,0)→(100,0)→(100,50), perp=x, along=y
        // seg0: x:0→100 (0!=100, skip), seg1: x:100→100 (==, len=|50-0|=50) → lane=100
        let r = route(vec![(0.0, 0.0), (100.0, 0.0), (100.0, 50.0)]);
        assert_eq!(gutter_lane_of(&r, "x", "y"), Some(100.0));
    }

    #[test]
    fn gutter_lane_of_l_shape_perp_y() {
        // Node: L=(0,0)→(100,0)→(100,50), perp=y, along=x
        // seg0: y:0→0 (==, len=|100-0|=100) → lane=0 (best)
        // seg1: y:0→50 (skip) → lane=0
        let r = route(vec![(0.0, 0.0), (100.0, 0.0), (100.0, 50.0)]);
        assert_eq!(gutter_lane_of(&r, "y", "x"), Some(0.0));
    }

    #[test]
    fn gutter_lane_of_staircase_picks_longest() {
        // Node: 4-pt=(0,0)→(50,0)→(50,80)→(200,80), perp=y, along=x
        // seg0: y=0→0 (==, len=50), seg1: y=0→80 (skip), seg2: y=80→80 (==, len=150)
        // → best is seg2 (len=150), lane=80
        let r = route(vec![(0.0, 0.0), (50.0, 0.0), (50.0, 80.0), (200.0, 80.0)]);
        assert_eq!(gutter_lane_of(&r, "y", "x"), Some(80.0));
    }

    #[test]
    fn gutter_lane_of_staircase_perp_x() {
        // Node: 4-pt=(0,0)→(50,0)→(50,80)→(200,80), perp=x, along=y
        // seg0: x=0→50 (skip), seg1: x=50→50 (==, len=80), seg2: x=50→200 (skip)
        // → lane=50
        let r = route(vec![(0.0, 0.0), (50.0, 0.0), (50.0, 80.0), (200.0, 80.0)]);
        assert_eq!(gutter_lane_of(&r, "x", "y"), Some(50.0));
    }

    #[test]
    fn gutter_lane_of_single_point_none() {
        let r = route(vec![(0.0, 0.0)]);
        assert_eq!(gutter_lane_of(&r, "x", "y"), None);
    }

    #[test]
    fn gutter_lane_of_empty_none() {
        let r = route(vec![]);
        assert_eq!(gutter_lane_of(&r, "x", "y"), None);
    }

    #[test]
    fn gutter_lane_of_straight_horizontal() {
        // Node: straight horizontal (0,50)→(100,50), perp=y, along=x
        // seg0: y=50→50 (==, len=100) → lane=50
        let r = route(vec![(0.0, 50.0), (100.0, 50.0)]);
        assert_eq!(gutter_lane_of(&r, "y", "x"), Some(50.0));
        // perp=x: x=0→100 (skip) → None
        assert_eq!(gutter_lane_of(&r, "x", "y"), None);
    }
}
