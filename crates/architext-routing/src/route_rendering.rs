//! Faithful port of `viewer/src/routing/routeRendering.js`.
//!
//! Builds the SVG `d`-path strings that are hashed byte-for-byte by the parity
//! fingerprint harness. Every coordinate written into a `d` string goes through
//! `js_number_to_string` to reproduce V8 template-literal formatting exactly.

use crate::js_compat::{js_number_to_string, js_sign};
use crate::model::Point;

// ---------------------------------------------------------------------------
// Constants (mirrors the JS module-level consts)
// ---------------------------------------------------------------------------

pub const HOP_RADIUS: f64 = 6.0;
const MIN_HOP_RADIUS: f64 = 2.0;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A crossing between two orthogonal segments. Carries `x`, `y`, `radius`, and
/// `direction` (±1: the sign of travel along the crossing segment).
#[derive(Debug, Clone)]
pub struct Crossing {
    x: f64,
    y: f64,
    radius: f64,
    direction: f64,
}

// ---------------------------------------------------------------------------
// Route input abstraction
//
// JS `pathToSvgWithHops` accepts `previousRoutes` as either plain `[Point]`
// arrays or objects with a `.points` property. We model this with an enum so
// `isSameRoute` can replicate the `route === points || route?.points === points`
// identity check.
// ---------------------------------------------------------------------------

/// Mirrors the two shapes the JS `previousRoutes` array can contain.
pub enum RouteRef<'a> {
    /// A bare `&[Point]` slice — JS array form.
    Points(&'a [Point]),
    /// A route object carrying a `.points` slice — JS object form.
    WithPoints(&'a [Point]),
}

impl<'a> RouteRef<'a> {
    fn points(&self) -> &'a [Point] {
        match self {
            RouteRef::Points(pts) => pts,
            RouteRef::WithPoints(pts) => pts,
        }
    }
}

// ---------------------------------------------------------------------------
// horizontalVerticalIntersection
// ---------------------------------------------------------------------------

/// Port of `horizontalVerticalIntersection`. Returns `Some(Crossing)` when the
/// two segments have an interior crossing with at least `MIN_HOP_RADIUS` of
/// room; `None` otherwise (boundary touch or too close to a corner).
///
/// `horizontal_start`/`horizontal_end` must be the horizontal segment;
/// `vertical_start`/`vertical_end` must be the vertical segment.
pub fn horizontal_vertical_intersection(
    horizontal_start: &Point,
    horizontal_end: &Point,
    vertical_start: &Point,
    vertical_end: &Point,
) -> Option<Crossing> {
    let min_x = horizontal_start.x.min(horizontal_end.x);
    let max_x = horizontal_start.x.max(horizontal_end.x);
    let min_y = vertical_start.y.min(vertical_end.y);
    let max_y = vertical_start.y.max(vertical_end.y);
    let x = vertical_start.x;
    let y = horizontal_start.y;

    if x <= min_x || x >= max_x || y <= min_y || y >= max_y {
        return None; // not an interior crossing
    }

    let radius = HOP_RADIUS
        .min(x - min_x)
        .min(max_x - x)
        .min(y - min_y)
        .min(max_y - y);
    if radius < MIN_HOP_RADIUS {
        return None;
    }
    // direction is filled in by the caller; leave 0.0 here as a sentinel.
    Some(Crossing { x, y, radius, direction: 0.0 })
}

// ---------------------------------------------------------------------------
// Internal helpers (private)
// ---------------------------------------------------------------------------

/// Port of `mergeCollinearPoints`. Collapses redundant collinear waypoints into
/// maximal straight runs so that crossings landing on intermediate waypoints
/// are still detected as interior.
fn merge_collinear_points(points: &[Point]) -> Vec<Point> {
    if points.is_empty() {
        return vec![];
    }
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut merged: Vec<Point> = vec![points[0].clone()];
    for index in 1..points.len() - 1 {
        let previous = &merged[merged.len() - 1];
        let current = &points[index];
        let next = &points[index + 1];
        let collinear = (previous.x == current.x && current.x == next.x)
            || (previous.y == current.y && current.y == next.y);
        if !collinear {
            merged.push(current.clone());
        }
    }
    merged.push(points[points.len() - 1].clone());
    merged
}

/// Port of `orthogonalCrossings`. Returns a map from segment index → list of
/// crossings on that segment, in insertion order (matching JS `Map` iteration).
fn orthogonal_crossings(
    points: &[Point],
    other_polylines: &[Vec<Point>],
) -> indexmap::IndexMap<usize, Vec<Crossing>> {
    let mut crossings: indexmap::IndexMap<usize, Vec<Crossing>> = indexmap::IndexMap::new();

    for index in 0..points.len().saturating_sub(1) {
        let start = &points[index];
        let end = &points[index + 1];
        // Skip diagonal segments.
        if start.x != end.x && start.y != end.y {
            continue;
        }

        for other_points in other_polylines {
            if other_points.is_empty() {
                continue;
            }
            for used_index in 0..other_points.len().saturating_sub(1) {
                let used_start = &other_points[used_index];
                let used_end = &other_points[used_index + 1];
                // Skip diagonal other-segments.
                if used_start.x != used_end.x && used_start.y != used_end.y {
                    continue;
                }

                if start.x == end.x && used_start.y == used_end.y {
                    // Self is vertical, other is horizontal.
                    if let Some(mut crossing) =
                        horizontal_vertical_intersection(used_start, used_end, start, end)
                    {
                        let dy = end.y - start.y;
                        crossing.direction = if dy > 0.0 { 1.0 } else if dy < 0.0 { -1.0 } else { 1.0 };
                        crossings.entry(index).or_default().push(crossing);
                    }
                } else if start.y == end.y && used_start.x == used_end.x {
                    // Self is horizontal, other is vertical.
                    if let Some(mut crossing) =
                        horizontal_vertical_intersection(start, end, used_start, used_end)
                    {
                        let dx = end.x - start.x;
                        crossing.direction = if dx > 0.0 { 1.0 } else if dx < 0.0 { -1.0 } else { 1.0 };
                        crossings.entry(index).or_default().push(crossing);
                    }
                }
            }
        }
    }
    crossings
}

// ---------------------------------------------------------------------------
// collapseBacktrackingPoints
// ---------------------------------------------------------------------------

/// Port of `collapseBacktrackingPoints`. Repeatedly removes the middle point of
/// any U-turn triple until no more exist.
fn collapse_backtracking_points(points: Vec<Point>) -> Vec<Point> {
    let mut collapsed = points;
    let mut changed = true;
    while changed {
        changed = false;
        for index in 1..collapsed.len().saturating_sub(1) {
            let previous = &collapsed[index - 1];
            let current = &collapsed[index];
            let next = &collapsed[index + 1];
            // `js_sign` (Math.sign), NOT `f64::signum`: Math.sign(0)===0, so a
            // zero delta (two equal consecutive points) is not a reversal. With
            // signum (±1.0 for ±0.0) Rust over-collapsed a trailing duplicate
            // point JS keeps — the bundle-coaligned 10-vs-1-crossing divergence.
            let horizontal_backtrack = previous.y == current.y
                && current.y == next.y
                && js_sign(current.x - previous.x) == -js_sign(next.x - current.x);
            let vertical_backtrack = previous.x == current.x
                && current.x == next.x
                && js_sign(current.y - previous.y) == -js_sign(next.y - current.y);
            if horizontal_backtrack || vertical_backtrack {
                collapsed.remove(index);
                changed = true;
                break;
            }
        }
    }
    collapsed
}

// ---------------------------------------------------------------------------
// Public exports
// ---------------------------------------------------------------------------

/// Port of `pathToSvg`. Builds a plain `M … L … L …` path string with no hop
/// arcs. Every coordinate goes through `js_number_to_string`.
pub fn path_to_svg(points: &[Point]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let cmd = if index == 0 { "M" } else { "L" };
            format!(
                "{} {} {}",
                cmd,
                js_number_to_string(point.x),
                js_number_to_string(point.y)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Port of `pathToSvgWithHops`. Builds an SVG `d`-path for `points`, hopping
/// over any segments from `previous_routes` that cross it orthogonally.
///
/// `previous_routes`: an ordered slice of route refs. Each ref is checked with
/// `isSameRoute` logic and skipped if it refers to the same underlying point
/// slice as `points`.
pub fn path_to_svg_with_hops(points: &[Point], previous_routes: &[RouteRef<'_>]) -> String {
    let self_pts = merge_collinear_points(points);

    // Gather other polylines, skipping self (isSameRoute logic: skip if same slice ptr).
    let mut other_polylines: Vec<Vec<Point>> = Vec::new();
    for route in previous_routes {
        // isSameRoute: route === points (bare array) or route?.points === points (object)
        // In our model: RouteRef::Points(ptr) is the "array" form; both forms compare
        // the inner slice pointer to `points`.
        let other_raw = route.points();
        if std::ptr::eq(other_raw, points) {
            continue;
        }
        // Also check the points-as-array case: if the inner slice is the same object.
        // (JS: `route === points` covers the bare-array case where route *is* the points array.)
        if !other_raw.is_empty() {
            other_polylines.push(merge_collinear_points(other_raw));
        }
    }

    let crossings = orthogonal_crossings(&self_pts, &other_polylines);
    if crossings.is_empty() {
        return path_to_svg(&self_pts);
    }

    // Build commands with hop arcs.
    let mut commands: Vec<String> = Vec::new();
    if !self_pts.is_empty() {
        commands.push(format!(
            "M {} {}",
            js_number_to_string(self_pts[0].x),
            js_number_to_string(self_pts[0].y)
        ));
    }

    for index in 0..self_pts.len().saturating_sub(1) {
        let start = &self_pts[index];
        let end = &self_pts[index + 1];

        // Sort crossings by distance from start along the segment.
        let mut segment_crossings: Vec<Crossing> =
            crossings.get(&index).cloned().unwrap_or_default();
        segment_crossings.sort_by(|a, b| {
            let dist_a;
            let dist_b;
            if start.x == end.x {
                dist_a = (a.y - start.y).abs();
                dist_b = (b.y - start.y).abs();
            } else {
                dist_a = (a.x - start.x).abs();
                dist_b = (b.x - start.x).abs();
            }
            // JS sort comparator: `(a, b) => dist_a - dist_b`.
            // f64::partial_cmp handles NaN as equal (shouldn't occur here).
            dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        for crossing in &segment_crossings {
            let radius = crossing.radius; // HOP_RADIUS already embedded by horizontalVerticalIntersection
            if start.y == end.y {
                // Horizontal segment — hop arcs upward (control point above the line).
                let before_x = crossing.x - crossing.direction * radius;
                let before_y = crossing.y;
                let after_x = crossing.x + crossing.direction * radius;
                let after_y = crossing.y;
                let ctrl_x = crossing.x;
                let ctrl_y = crossing.y - radius * 1.6;
                commands.push(format!(
                    "L {} {}",
                    js_number_to_string(before_x),
                    js_number_to_string(before_y)
                ));
                commands.push(format!(
                    "Q {} {} {} {}",
                    js_number_to_string(ctrl_x),
                    js_number_to_string(ctrl_y),
                    js_number_to_string(after_x),
                    js_number_to_string(after_y)
                ));
            } else {
                // Vertical segment — hop arcs rightward (control point to the right).
                let before_x = crossing.x;
                let before_y = crossing.y - crossing.direction * radius;
                let after_x = crossing.x;
                let after_y = crossing.y + crossing.direction * radius;
                let ctrl_x = crossing.x + radius * 1.6;
                let ctrl_y = crossing.y;
                commands.push(format!(
                    "L {} {}",
                    js_number_to_string(before_x),
                    js_number_to_string(before_y)
                ));
                commands.push(format!(
                    "Q {} {} {} {}",
                    js_number_to_string(ctrl_x),
                    js_number_to_string(ctrl_y),
                    js_number_to_string(after_x),
                    js_number_to_string(after_y)
                ));
            }
        }

        commands.push(format!(
            "L {} {}",
            js_number_to_string(end.x),
            js_number_to_string(end.y)
        ));
    }

    commands.join(" ")
}

/// Port of `simplifyOrthogonalPoints`. Deduplicates, collapses collinear runs
/// (preserving the port-stub elbow at index 2 of the deduped sequence and the
/// last point), then collapses any backtracking U-turns.
pub fn simplify_orthogonal_points(points: &[Point]) -> Vec<Point> {
    const PORT_STUB_ELBOW_INDEX: usize = 2;

    // Step 1: deduplicate consecutive identical points.
    let mut deduped: Vec<Point> = Vec::new();
    for point in points {
        match deduped.last() {
            Some(prev) if prev.x == point.x && prev.y == point.y => {}
            _ => deduped.push(point.clone()),
        }
    }

    // Step 2: collapse collinear triples (except at the protected elbow index and last).
    let mut simplified: Vec<Point> = Vec::new();
    for index in 0..deduped.len() {
        let point = &deduped[index];
        let is_last = index == deduped.len() - 1;
        let is_elbow = index == PORT_STUB_ELBOW_INDEX;

        if !is_elbow && !is_last && simplified.len() >= 2 {
            let previous = &simplified[simplified.len() - 1];
            let before_previous = &simplified[simplified.len() - 2];
            let collinear = (before_previous.x == previous.x && previous.x == point.x)
                || (before_previous.y == previous.y && previous.y == point.y);
            if collinear {
                *simplified.last_mut().unwrap() = point.clone();
                continue;
            }
        }
        simplified.push(point.clone());
    }

    // Step 3: collapse backtracking U-turns.
    collapse_backtracking_points(simplified)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Point;

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    // --- pathToSvg ---

    #[test]
    fn path_to_svg_straight_integers() {
        // Node: pathToSvg([{x:10,y:20},{x:50,y:20},{x:50,y:60}])
        // => "M 10 20 L 50 20 L 50 60"
        let pts = vec![p(10.0, 20.0), p(50.0, 20.0), p(50.0, 60.0)];
        assert_eq!(path_to_svg(&pts), "M 10 20 L 50 20 L 50 60");
    }

    #[test]
    fn path_to_svg_fractional_coords() {
        // Node: pathToSvg([{x:10.5,y:20.3},{x:50.7,y:20.3}])
        // => "M 10.5 20.3 L 50.7 20.3"
        let pts = vec![p(10.5, 20.3), p(50.7, 20.3)];
        assert_eq!(path_to_svg(&pts), "M 10.5 20.3 L 50.7 20.3");
    }

    #[test]
    fn path_to_svg_empty() {
        assert_eq!(path_to_svg(&[]), "");
    }

    #[test]
    fn path_to_svg_single_point() {
        // Node: pathToSvg([{x:5,y:10}]) => "M 5 10"
        assert_eq!(path_to_svg(&[p(5.0, 10.0)]), "M 5 10");
    }

    // --- horizontal_vertical_intersection ---

    #[test]
    fn hvi_valid_interior_crossing() {
        // Node: horizontalVerticalIntersection({x:0,y:50},{x:200,y:50},{x:100,y:0},{x:100,y:100})
        // => {x:100, y:50, radius:6}
        let result = horizontal_vertical_intersection(
            &p(0.0, 50.0),
            &p(200.0, 50.0),
            &p(100.0, 0.0),
            &p(100.0, 100.0),
        );
        assert!(result.is_some());
        let c = result.unwrap();
        assert_eq!(c.x, 100.0);
        assert_eq!(c.y, 50.0);
        assert_eq!(c.radius, 6.0);
    }

    #[test]
    fn hvi_boundary_touch_returns_none() {
        // Node: horizontalVerticalIntersection({x:0,y:50},{x:100,y:50},{x:100,y:0},{x:100,y:100})
        // => null  (x == maxX, not interior)
        let result = horizontal_vertical_intersection(
            &p(0.0, 50.0),
            &p(100.0, 50.0),
            &p(100.0, 0.0),
            &p(100.0, 100.0),
        );
        assert!(result.is_none());
    }

    #[test]
    fn hvi_adaptive_radius() {
        // Node: horizontalVerticalIntersection({x:0,y:50},{x:200,y:50},{x:3,y:0},{x:3,y:100})
        // => {x:3, y:50, radius:3}
        let result = horizontal_vertical_intersection(
            &p(0.0, 50.0),
            &p(200.0, 50.0),
            &p(3.0, 0.0),
            &p(3.0, 100.0),
        );
        assert!(result.is_some());
        let c = result.unwrap();
        assert_eq!(c.x, 3.0);
        assert_eq!(c.y, 50.0);
        assert_eq!(c.radius, 3.0);
    }

    #[test]
    fn hvi_below_min_radius_returns_none() {
        // Node: horizontalVerticalIntersection({x:0,y:50},{x:200,y:50},{x:1,y:0},{x:1,y:100})
        // => null  (room = 1, < MIN_HOP_RADIUS=2)
        let result = horizontal_vertical_intersection(
            &p(0.0, 50.0),
            &p(200.0, 50.0),
            &p(1.0, 0.0),
            &p(1.0, 100.0),
        );
        assert!(result.is_none());
    }

    #[test]
    fn hvi_exactly_min_radius_renders() {
        // Node: horizontalVerticalIntersection({x:0,y:50},{x:200,y:50},{x:2,y:0},{x:2,y:100})
        // => {x:2, y:50, radius:2}
        let result = horizontal_vertical_intersection(
            &p(0.0, 50.0),
            &p(200.0, 50.0),
            &p(2.0, 0.0),
            &p(2.0, 100.0),
        );
        assert!(result.is_some());
        let c = result.unwrap();
        assert_eq!(c.radius, 2.0);
    }

    // --- pathToSvgWithHops ---

    #[test]
    fn path_to_svg_with_hops_no_crossings() {
        // Node: pathToSvgWithHops([{x:10,y:20},{x:100,y:20}], [])
        // => "M 10 20 L 100 20"
        let pts = vec![p(10.0, 20.0), p(100.0, 20.0)];
        assert_eq!(path_to_svg_with_hops(&pts, &[]), "M 10 20 L 100 20");
    }

    #[test]
    fn path_to_svg_with_hops_empty() {
        // Node: pathToSvgWithHops([], []) => ""
        assert_eq!(path_to_svg_with_hops(&[], &[]), "");
    }

    #[test]
    fn path_to_svg_with_hops_single_point() {
        // Node: pathToSvgWithHops([{x:10,y:20}], []) => "M 10 20"
        let pts = vec![p(10.0, 20.0)];
        assert_eq!(path_to_svg_with_hops(&pts, &[]), "M 10 20");
    }

    #[test]
    fn path_to_svg_with_hops_horizontal_crossing() {
        // Route A: horizontal (0,50)→(200,50)
        // Route B: vertical (100,0)→(100,100)  — crosses A at (100,50)
        // Node: "M 0 50 L 94 50 Q 100 40.4 106 50 L 200 50"
        // radius=6, direction=+1 (A goes right)
        // before=(94,50), ctrl=(100, 50-6*1.6=40.4), after=(106,50)
        let route_a = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route_b = vec![p(100.0, 0.0), p(100.0, 100.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 0 50 L 94 50 Q 100 40.4 106 50 L 200 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_vertical_crossing_downward() {
        // Route A: vertical (100,0)→(100,200)
        // Route B: horizontal (0,50)→(200,50) — crosses A at (100,50)
        // direction=+1 (A goes down), before=(100,44), ctrl=(100+6*1.6=109.6, 50), after=(100,56)
        // Node: "M 100 0 L 100 44 Q 109.6 50 100 56 L 100 200"
        let route_a = vec![p(100.0, 0.0), p(100.0, 200.0)];
        let route_b = vec![p(0.0, 50.0), p(200.0, 50.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 100 0 L 100 44 Q 109.6 50 100 56 L 100 200"
        );
    }

    #[test]
    fn path_to_svg_with_hops_vertical_crossing_upward() {
        // Route A: vertical (100,200)→(100,0)  — going UP, direction = -1
        // Route B: horizontal (0,50)→(200,50)
        // before=(100, 50-(-1)*6=56), ctrl=(109.6, 50), after=(100, 50+(-1)*6=44)
        // Node: "M 100 200 L 100 56 Q 109.6 50 100 44 L 100 0"
        let route_a = vec![p(100.0, 200.0), p(100.0, 0.0)];
        let route_b = vec![p(0.0, 50.0), p(200.0, 50.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 100 200 L 100 56 Q 109.6 50 100 44 L 100 0"
        );
    }

    #[test]
    fn path_to_svg_with_hops_horizontal_leftward() {
        // Route A: horizontal (200,50)→(0,50)  — going LEFT, direction = -1
        // Route B: vertical (100,0)→(100,200)
        // before=(100-(-1)*6=106, 50), ctrl=(100, 50-6*1.6=40.4), after=(100+(-1)*6=94, 50)
        // Node: "M 200 50 L 106 50 Q 100 40.4 94 50 L 0 50"
        let route_a = vec![p(200.0, 50.0), p(0.0, 50.0)];
        let route_b = vec![p(100.0, 0.0), p(100.0, 200.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 200 50 L 106 50 Q 100 40.4 94 50 L 0 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_fractional_radius() {
        // Route A: horizontal (0,50)→(200,50)
        // Route B: vertical (3,0)→(3,100)  — adaptive radius=3, 3*1.6=4.800000000000001
        // before=(3-1*3=0, 50), ctrl=(3, 50-3*1.6=45.2), after=(6, 50)
        // Node: "M 0 50 L 0 50 Q 3 45.2 6 50 L 200 50"
        // Note: 3*1.6 in f64 = 4.800000000000001, but 50-4.800000000000001 = 45.2 (exact)
        let route_a = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route_b = vec![p(3.0, 0.0), p(3.0, 100.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 0 50 L 0 50 Q 3 45.2 6 50 L 200 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_multiple_crossings_sorted() {
        // Route A: horizontal (0,50)→(300,50)
        // Routes G: vertical (200,0)→(200,100) and H: vertical (100,0)→(100,100)
        // G added first (farther), H second (closer) — sort must order by distance from start
        // Expected: hop at x=100 first, then x=200
        // Node: "M 0 50 L 94 50 Q 100 40.4 106 50 L 194 50 Q 200 40.4 206 50 L 300 50"
        let route_a = vec![p(0.0, 50.0), p(300.0, 50.0)];
        let route_g = vec![p(200.0, 0.0), p(200.0, 100.0)];
        let route_h = vec![p(100.0, 0.0), p(100.0, 100.0)];
        assert_eq!(
            path_to_svg_with_hops(
                &route_a,
                &[RouteRef::Points(&route_g), RouteRef::Points(&route_h)]
            ),
            "M 0 50 L 94 50 Q 100 40.4 106 50 L 194 50 Q 200 40.4 206 50 L 300 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_collinear_merge_enables_crossing() {
        // Route A: (0,50)→(100,50)→(200,50) — collinear midpoint at x=100
        // Route B: vertical (150,0)→(150,100)
        // Merge makes a single segment, crossing at x=150 is interior
        // Node: "M 0 50 L 144 50 Q 150 40.4 156 50 L 200 50"
        let route_a = vec![p(0.0, 50.0), p(100.0, 50.0), p(200.0, 50.0)];
        let route_b = vec![p(150.0, 0.0), p(150.0, 100.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 0 50 L 144 50 Q 150 40.4 156 50 L 200 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_route_object_form() {
        // previousRoutes entry is WithPoints (JS object with .points)
        // Node: "M 0 50 L 94 50 Q 100 40.4 106 50 L 200 50"
        let route_a = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route_b_pts = vec![p(100.0, 0.0), p(100.0, 100.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::WithPoints(&route_b_pts)]),
            "M 0 50 L 94 50 Q 100 40.4 106 50 L 200 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_skips_self_by_ptr() {
        // When previousRoutes contains the same slice as points, it must be skipped.
        // Node: "M 0 50 L 200 50"  (no crossing with self)
        let route_a = vec![p(0.0, 50.0), p(200.0, 50.0)];
        // Pass RouteRef::Points(&route_a) — same ptr as route_a
        let refs = [RouteRef::Points(&route_a)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &refs),
            "M 0 50 L 200 50"
        );
    }

    #[test]
    fn path_to_svg_with_hops_adaptive_hop_close_to_end() {
        // Route A: horizontal (0,50)→(105,50)
        // Route B: vertical (100,0)→(100,100) — room on right = 105-100=5, radius=min(6,100,5,50,50)=5
        // Node: "M 0 50 L 95 50 Q 100 42 105 50 L 105 50"
        // 5*1.6=8.0 exactly, ctrl_y=50-8=42
        let route_a = vec![p(0.0, 50.0), p(105.0, 50.0)];
        let route_b = vec![p(100.0, 0.0), p(100.0, 100.0)];
        assert_eq!(
            path_to_svg_with_hops(&route_a, &[RouteRef::Points(&route_b)]),
            "M 0 50 L 95 50 Q 100 42 105 50 L 105 50"
        );
    }

    // --- simplifyOrthogonalPoints ---

    #[test]
    fn simplify_deduplicates_consecutive_identical() {
        // Node: simplifyOrthogonalPoints([{x:0,y:0},{x:10,y:0},{x:10,y:0},{x:50,y:0}])
        // => [{x:0,y:0},{x:10,y:0},{x:50,y:0}]
        let pts = vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 0.0), p(50.0, 0.0)];
        let result = simplify_orthogonal_points(&pts);
        let expected = vec![p(0.0, 0.0), p(10.0, 0.0), p(50.0, 0.0)];
        assert_eq!(result, expected);
    }

    #[test]
    fn simplify_backtracking() {
        // Node: simplifyOrthogonalPoints([{x:0,y:0},{x:50,y:0},{x:30,y:0},{x:100,y:0}])
        // => [{x:0,y:0},{x:30,y:0},{x:100,y:0}]
        let pts = vec![p(0.0, 0.0), p(50.0, 0.0), p(30.0, 0.0), p(100.0, 0.0)];
        let result = simplify_orthogonal_points(&pts);
        let expected = vec![p(0.0, 0.0), p(30.0, 0.0), p(100.0, 0.0)];
        assert_eq!(result, expected);
    }

    #[test]
    fn simplify_two_points_unchanged() {
        // Node: simplifyOrthogonalPoints([{x:0,y:0},{x:50,y:0}]) => [{x:0,y:0},{x:50,y:0}]
        let pts = vec![p(0.0, 0.0), p(50.0, 0.0)];
        let result = simplify_orthogonal_points(&pts);
        assert_eq!(result, pts);
    }

    #[test]
    fn simplify_single_point_unchanged() {
        let pts = vec![p(0.0, 0.0)];
        let result = simplify_orthogonal_points(&pts);
        assert_eq!(result, pts);
    }

    #[test]
    fn simplify_long_collinear() {
        // Node: simplifyOrthogonalPoints([{x:0,y:0},{x:0,y:10},{x:0,y:20},{x:0,y:30},{x:50,y:30}])
        // => [{x:0,y:0},{x:0,y:10},{x:0,y:30},{x:50,y:30}]
        // index=0: push {0,0}
        // index=1 (portStubElbow=2? no, 1≠2): push {0,10}
        // index=2 (portStubElbow=2? YES): push {0,20}
        // index=3 (not last, not elbow): prev={0,20}, beforePrev={0,10}, collinear y==y==y -> replace -> {0,30}
        // index=4 (last): push {50,30}
        let pts = vec![
            p(0.0, 0.0),
            p(0.0, 10.0),
            p(0.0, 20.0),
            p(0.0, 30.0),
            p(50.0, 30.0),
        ];
        let result = simplify_orthogonal_points(&pts);
        let expected = vec![p(0.0, 0.0), p(0.0, 10.0), p(0.0, 30.0), p(50.0, 30.0)];
        assert_eq!(result, expected);
    }

    #[test]
    fn simplify_elbow_at_index2_preserved() {
        // Node: simplifyOrthogonalPoints([{x:0,y:0},{x:0,y:20},{x:50,y:20},{x:50,y:40},{x:100,y:40}])
        // => same (no collinear runs, no backtracks)
        let pts = vec![
            p(0.0, 0.0),
            p(0.0, 20.0),
            p(50.0, 20.0),
            p(50.0, 40.0),
            p(100.0, 40.0),
        ];
        let result = simplify_orthogonal_points(&pts);
        assert_eq!(result, pts);
    }
}
