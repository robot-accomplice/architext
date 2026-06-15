//! Faithful port of the helper-function surface of
//! `viewer/src/routing/routeEdges.js` that `routeMountModel.js` imports.
//!
//! Ported functions (Pass A — routeMountModel dependency surface):
//!   endpointSide, axisAlignedSegments, sharedSegmentLength,
//!   sideNeedsPostSelectionCentering, routeCollidesWithNonEndpoints,
//!   routeHasEndpointTraversal, offsetEndpointRoute, endpointSpreadOffset,
//!   routeWithPoints, offsetOrthogonalPolyline.
//!
//! Private helpers ported because the above 10 functions transitively require
//! them (all stay within this subset — none drag in the full orchestration):
//!   orthogonalizedPoints, endpointOffsetPoints.
//!
//! NOT ported in this pass: routeEdges() orchestration, crossingsBetween,
//! candidate-selection pipeline, mount-pass invocation.
//!
//! Translation decisions:
//! - JS `route` is a generic dict spread by every caller. We model it as a
//!   struct `RouteData` carrying the fields all 10 functions read/write.
//!   The `extra` bag carries fields we do not need to inspect here.
//! - `Math.round` → `js_compat::js_round` (not used directly by these 10,
//!   but kept available for callers).
//! - `Math.hypot` → `crate::js_compat::js_hypot` (not used in this surface).
//! - `Math.min/max` on f64 → `f64::min/f64::max` (exact same IEEE semantics).
//! - JS template-literal coordinate formatting in `d` paths →
//!   `crate::js_compat::js_number_to_string`.
//! - `simplifyOrthogonalPoints` lives in `crate::route_rendering`.
//! - `boundsForPoints`, `bend_count`, `line_samples`, `sample_cubic`,
//!   `sample_line` live in `crate::route_geometry`.
//! - `anchor_for`, `port_for` live in `crate::route_ports`.
//! - `routeIntersectsRect` is reproduced locally (only orthogonal branch used
//!   by `routeCollidesWithNonEndpoints` / `routeHasEndpointTraversal`).
//! - IndexMap/IndexSet not needed in this surface (no Map/Set iteration for
//!   decisions here — all decisions are over vecs or plain fields).

use crate::js_compat::js_number_to_string;
use crate::model::{Point, Rect};
use crate::route_geometry::{
    bend_count, bounds_for_points, line_samples, sample_cubic, sample_line,
    segment_intersects_rect,
};
use crate::route_ports::{anchor_for, port_for};
use crate::route_rendering::simplify_orthogonal_points;
use indexmap::IndexMap;

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
// endpointSide
// ---------------------------------------------------------------------------

/// Port of JS `endpointSide(rect, point)`.
///
/// Returns "left", "right", "top", "bottom", or "" when `point` does not lie
/// on any boundary of `rect`.
pub fn endpoint_side(rect: &Rect, point: &Point) -> &'static str {
    if point.x == rect.x {
        return "left";
    }
    if point.x == rect.x + rect.width {
        return "right";
    }
    if point.y == rect.y {
        return "top";
    }
    if point.y == rect.y + rect.height {
        return "bottom";
    }
    ""
}

// ---------------------------------------------------------------------------
// sideNeedsPostSelectionCentering
// ---------------------------------------------------------------------------

/// Port of JS `sideNeedsPostSelectionCentering(side)`.
///
/// Returns `true` for any of the four cardinal sides; `false` for "" or any
/// other string (which can arise when a point is not on any side).
pub fn side_needs_post_selection_centering(side: &str) -> bool {
    matches!(side, "left" | "right" | "top" | "bottom")
}

// ---------------------------------------------------------------------------
// axisAlignedSegments
// ---------------------------------------------------------------------------

/// An axis-aligned segment extracted from a route's point list.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisAlignedSegment {
    pub orientation: &'static str, // "horizontal" | "vertical"
    /// The constant coordinate: `x` for vertical, `y` for horizontal.
    pub line: f64,
    /// Minimum of the varying coordinate.
    pub min: f64,
    /// Maximum of the varying coordinate.
    pub max: f64,
}

/// Port of JS `axisAlignedSegments(route)`.
///
/// Extracts all horizontal and vertical segments from `route.points`. Diagonal
/// segments (neither axis-aligned) are skipped, matching the JS behavior.
pub fn axis_aligned_segments(route: &RouteData) -> Vec<AxisAlignedSegment> {
    let points = &route.points;
    let mut segments = Vec::new();
    for index in 0..points.len().saturating_sub(1) {
        let start = &points[index];
        let end = &points[index + 1];
        if start.x == end.x {
            segments.push(AxisAlignedSegment {
                orientation: "vertical",
                line: start.x,
                min: f64::min(start.y, end.y),
                max: f64::max(start.y, end.y),
            });
        } else if start.y == end.y {
            segments.push(AxisAlignedSegment {
                orientation: "horizontal",
                line: start.y,
                min: f64::min(start.x, end.x),
                max: f64::max(start.x, end.x),
            });
        }
    }
    segments
}

// ---------------------------------------------------------------------------
// sharedSegmentLength
// ---------------------------------------------------------------------------

/// Port of JS `sharedSegmentLength(left, right)`.
///
/// Returns the overlap length between two axis-aligned segments, or 0 if they
/// are in different orientations or on different lines.
pub fn shared_segment_length(left: &AxisAlignedSegment, right: &AxisAlignedSegment) -> f64 {
    if left.orientation != right.orientation {
        return 0.0;
    }
    if left.line != right.line {
        return 0.0;
    }
    f64::max(0.0, f64::min(left.max, right.max) - f64::max(left.min, right.min))
}

// ---------------------------------------------------------------------------
// routeIntersectsRect — private helper (used by collision functions below)
// ---------------------------------------------------------------------------

/// Port of the orthogonal branch of JS `routeIntersectsRect(route, rect, padding)`.
///
/// Only the orthogonal (`points`-based) branch is used by the functions ported
/// here. The spline/straight sample branch is not needed by this surface.
fn route_intersects_rect(route: &RouteData, rect: &Rect, padding: f64) -> bool {
    // The JS function checks `route.sampleBounds` for an early-exit culling.
    // We replicate it using `sample_bounds` from our RouteData.
    let sample_bounds = &route.sample_bounds;
    // rectsOverlap check (mirrors JS rectsOverlap):
    if sample_bounds.width > 0.0 || sample_bounds.height > 0.0 {
        let a = sample_bounds;
        let b = rect;
        let no_overlap = a.x + a.width + padding < b.x
            || b.x + b.width + padding < a.x
            || a.y + a.height + padding < b.y
            || b.y + b.height + padding < a.y;
        if no_overlap {
            return false;
        }
    }
    if route.style == "orthogonal" && !route.points.is_empty() {
        for index in 0..route.points.len().saturating_sub(1) {
            if segment_intersects_rect(&route.points[index], &route.points[index + 1], rect, padding) {
                return true;
            }
        }
        return false;
    }
    // Fallback: sample-based check (for spline/straight routes).
    route.samples.iter().any(|p| {
        p.x > rect.x - padding
            && p.x < rect.x + rect.width + padding
            && p.y > rect.y - padding
            && p.y < rect.y + rect.height + padding
    })
}

// ---------------------------------------------------------------------------
// routeCollidesWithNonEndpoints
// ---------------------------------------------------------------------------

/// Port of JS `routeCollidesWithNonEndpoints(route, relationship, input)`.
///
/// Returns `true` if `route` intersects any visible node that is neither the
/// source (`relationship.from`) nor the target (`relationship.to`).
pub fn route_collides_with_non_endpoints(
    route: &RouteData,
    relationship: &Relationship<'_>,
    input: &RouteInput<'_>,
) -> bool {
    for node_id in input.visible_node_ids {
        if node_id == relationship.from || node_id == relationship.to {
            continue;
        }
        if let Some(rect) = input.node_rects.get(node_id.as_str()) {
            if route_intersects_rect(route, rect, 0.0) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// routeHasEndpointTraversal
// ---------------------------------------------------------------------------

/// Port of JS `routeHasEndpointTraversal(route, relationship, input)`.
///
/// Returns `true` if any sample point falls strictly inside the bounding box
/// of either endpoint node (`from` or `to`). Uses the `samples` array, matching
/// the JS strict-interior check (`>` / `<`, not `>=` / `<=`).
pub fn route_has_endpoint_traversal(
    route: &RouteData,
    relationship: &Relationship<'_>,
    input: &RouteInput<'_>,
) -> bool {
    for node_id in [relationship.from, relationship.to] {
        let rect = match input.node_rects.get(node_id) {
            Some(r) => r,
            None => continue,
        };
        let inside = route.samples.iter().any(|p| {
            p.x > rect.x
                && p.x < rect.x + rect.width
                && p.y > rect.y
                && p.y < rect.y + rect.height
        });
        if inside {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// endpointSpreadOffset
// ---------------------------------------------------------------------------

/// Port of JS `endpointSpreadOffset(index, count, rect, side)`.
///
/// Evenly spaces `count` mount points across the face whose length is
/// `rect.height` (left/right sides) or `rect.width` (top/bottom). Returns the
/// offset from the face centre for slot `index` of `count`.
pub fn endpoint_spread_offset(index: usize, count: usize, rect: &Rect, side: &str) -> f64 {
    let side_length = if side == "left" || side == "right" {
        rect.height
    } else {
        rect.width
    };
    ((index + 1) as f64 / (count + 1) as f64 - 0.5) * side_length
}

// ---------------------------------------------------------------------------
// orthogonalizedPoints — private helper
// ---------------------------------------------------------------------------

/// Port of JS private `orthogonalizedPoints(points)`.
///
/// Inserts a horizontal-then-vertical elbow between any two consecutive points
/// that are neither horizontally nor vertically aligned (i.e. "diagonal" steps
/// that the orthogonal router should not produce but could receive from callers).
fn orthogonalized_points(points: &[Point]) -> Vec<Point> {
    if points.is_empty() {
        return points.to_vec();
    }
    let mut next = vec![points[0].clone()];
    #[allow(clippy::needless_range_loop)]
    for index in 1..points.len() {
        let previous = next[next.len() - 1].clone();
        let point = &points[index];
        if previous.x != point.x && previous.y != point.y {
            // Insert horizontal-first elbow: move to (point.x, previous.y) first.
            next.push(Point { x: point.x, y: previous.y });
        }
        next.push(point.clone());
    }
    next
}

// ---------------------------------------------------------------------------
// routeWithPoints
// ---------------------------------------------------------------------------

/// Port of JS `routeWithPoints(route, points, controls = route.controls)`.
///
/// Rebuilds `d`, `points`, `controls`, `samples`, `sampleBounds`, `bends`,
/// `labelX`, `labelY` from the given `points` and optional `controls`.
///
/// For spline routes: points are used verbatim; if `controls` has exactly 2
/// entries the `d`-path is a cubic Bézier command and samples come from
/// `sample_cubic`.
/// For straight routes: `sample_line` with 18 steps.
/// For orthogonal and all others: `orthogonalized_points` →
/// `simplify_orthogonal_points` → `line_samples`.
///
/// The label is placed at the midpoint of `samples` (falling back to the
/// midpoint of `points`, then `(0,0)`).
pub fn route_with_points(
    route: &RouteData,
    points: Vec<Point>,
    controls: Option<[Point; 2]>,
) -> RouteData {
    let next_points: Vec<Point> = if route.style == "spline" {
        points
    } else {
        simplify_orthogonal_points(&orthogonalized_points(&points))
    };

    let samples: Vec<Point> = if route.style == "spline" {
        if let Some([ref c0, ref c1]) = controls {
            if let (Some(first), Some(last)) = (next_points.first(), next_points.last()) {
                let mut s = vec![first.clone()];
                s.extend(sample_cubic(first, c0, c1, last, 32));
                s
            } else {
                line_samples(&next_points)
            }
        } else {
            line_samples(&next_points)
        }
    } else if route.style == "straight" {
        if let (Some(first), Some(last)) = (next_points.first(), next_points.last()) {
            sample_line(first, last, 18)
        } else {
            line_samples(&next_points)
        }
    } else {
        line_samples(&next_points)
    };

    // label = samples[floor(samples.len/2)] ?? points[floor(points.len/2)] ?? {0,0}
    let mid_sample = samples.get(samples.len() / 2).cloned();
    let mid_point = next_points.get(next_points.len() / 2).cloned();
    let label = mid_sample
        .or(mid_point)
        .unwrap_or(Point { x: 0.0, y: 0.0 });

    // d-path string:
    // spline + 2 controls: "M x y C cx1 cy1 cx2 cy2 ex ey"
    // otherwise: "M x y L x y L x y ..."
    // Uses JS number-to-string for each coordinate, matching template literals.
    let d = if route.style == "spline" {
        if let Some([ref c0, ref c1]) = controls {
            if let (Some(first), Some(last)) = (next_points.first(), next_points.last()) {
                format!(
                    "M {} {} C {} {} {} {} {} {}",
                    js_number_to_string(first.x),
                    js_number_to_string(first.y),
                    js_number_to_string(c0.x),
                    js_number_to_string(c0.y),
                    js_number_to_string(c1.x),
                    js_number_to_string(c1.y),
                    js_number_to_string(last.x),
                    js_number_to_string(last.y),
                )
            } else {
                build_ml_path(&next_points)
            }
        } else {
            build_ml_path(&next_points)
        }
    } else {
        build_ml_path(&next_points)
    };

    let all_for_bounds: Vec<Point> = next_points
        .iter()
        .chain(samples.iter())
        .cloned()
        .collect();
    let sample_bounds = bounds_for_points(&all_for_bounds);
    let bends = bend_count(&next_points);

    RouteData {
        d,
        points: next_points,
        controls,
        samples,
        sample_bounds,
        bends,
        label_x: label.x,
        label_y: label.y,
        style: route.style.clone(),
        extra: route.extra.clone(),
    }
}

/// Build an "M x y L x y ..." path string using JS-faithful number formatting.
fn build_ml_path(points: &[Point]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let cmd = if i == 0 { "M" } else { "L" };
            format!(
                "{} {} {}",
                cmd,
                js_number_to_string(p.x),
                js_number_to_string(p.y)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// endpointOffsetPoints — private helper for offsetEndpointRoute
// ---------------------------------------------------------------------------

/// Port of JS private `endpointOffsetPoints(points, endpointIndex, rect, side, rawOffset)`.
///
/// Moves endpoint `endpointIndex` (treated as 0 → first, any other → last) to
/// the offset anchor on `side` of `rect`. Adjusts the adjacent and elbow points
/// to keep the route orthogonal. Returns the updated point list (may be longer
/// than the input for the 2-point case or when a new elbow must be inserted).
pub fn endpoint_offset_points(
    points: &[Point],
    endpoint_index: usize,
    rect: &Rect,
    side: &str,
    raw_offset: f64,
) -> Vec<Point> {
    let mut next: Vec<Point> = points.to_vec();
    let ep_idx = if endpoint_index == 0 { 0 } else { next.len() - 1 };
    let old_anchor = next[ep_idx].clone();
    let anchor = if raw_offset == 0.0 {
        anchor_for(rect, side)
    } else {
        port_for(rect, side, 18.0, raw_offset, false).anchor
    };
    next[ep_idx] = anchor.clone();
    let adjacent_idx = if ep_idx == 0 { 1 } else { next.len() - 2 };

    // 2-point case: insert port + elbow to keep orthogonality.
    if next.len() == 2 {
        let adjacent = next[adjacent_idx].clone();
        let port = if raw_offset == 0.0 {
            port_for(rect, side, 18.0, 0.0, false).port
        } else {
            port_for(rect, side, 18.0, raw_offset, false).port
        };
        let elbow = if side == "left" || side == "right" {
            Point { x: port.x, y: adjacent.y }
        } else {
            Point { x: adjacent.x, y: port.y }
        };
        return if ep_idx == 0 {
            vec![anchor, port, elbow, adjacent]
        } else {
            vec![adjacent, elbow, port, anchor]
        };
    }

    // Multi-point, rawOffset == 0 → centering pass.
    if raw_offset == 0.0 {
        if next.len() > 2 {
            let elbow_idx = if ep_idx == 0 { 2 } else { next.len() - 3 };
            if side == "top" || side == "bottom" {
                next[adjacent_idx].x = anchor.x;
                if next.get(elbow_idx).map(|p| p.x) == Some(old_anchor.x) {
                    next[elbow_idx].x = anchor.x;
                }
            } else {
                next[adjacent_idx].y = anchor.y;
                if next.get(elbow_idx).map(|p| p.y) == Some(old_anchor.y) {
                    next[elbow_idx].y = anchor.y;
                }
            }
        }
        return next;
    }

    // Multi-point, rawOffset != 0 → offset pass.
    if next.len() > 2 {
        let elbow_idx = if ep_idx == 0 { 2 } else { next.len() - 3 };
        // Note: adjacent is already in `next` (mutated below); capture beforeAdjacent now.
        let before_adjacent = next.get(elbow_idx).cloned();

        if side == "top" || side == "bottom" {
            next[adjacent_idx].x = anchor.x;
            let before_before_idx = if ep_idx == 0 {
                elbow_idx + 1
            } else if elbow_idx > 0 {
                elbow_idx - 1
            } else {
                usize::MAX
            };
            let before_before = if before_before_idx < next.len() {
                Some(next[before_before_idx].clone())
            } else {
                None
            };
            if let (Some(ba), Some(bb)) = (before_adjacent.as_ref(), before_before.as_ref()) {
                if bb.y == ba.y {
                    next[elbow_idx].x = next[adjacent_idx].x;
                } else if ba.x != next[adjacent_idx].x && ba.y != next[adjacent_idx].y {
                    let new_elbow = Point { x: next[adjacent_idx].x, y: ba.y };
                    if ep_idx == 0 {
                        next.insert(adjacent_idx + 1, new_elbow);
                    } else {
                        next.insert(adjacent_idx, new_elbow);
                    }
                }
            } else if let Some(ba) = before_adjacent.as_ref() {
                // No beforeBefore — same as the JS `else if` branch only when ba exists.
                if ba.x != next[adjacent_idx].x && ba.y != next[adjacent_idx].y {
                    let new_elbow = Point { x: next[adjacent_idx].x, y: ba.y };
                    if ep_idx == 0 {
                        next.insert(adjacent_idx + 1, new_elbow);
                    } else {
                        next.insert(adjacent_idx, new_elbow);
                    }
                }
            }
        } else {
            // left | right side
            next[adjacent_idx].y = anchor.y;
            let before_before_idx = if ep_idx == 0 {
                elbow_idx + 1
            } else if elbow_idx > 0 {
                elbow_idx - 1
            } else {
                usize::MAX
            };
            let before_before = if before_before_idx < next.len() {
                Some(next[before_before_idx].clone())
            } else {
                None
            };
            if let (Some(ba), Some(bb)) = (before_adjacent.as_ref(), before_before.as_ref()) {
                if bb.x == ba.x {
                    next[elbow_idx].y = next[adjacent_idx].y;
                } else if ba.x != next[adjacent_idx].x && ba.y != next[adjacent_idx].y {
                    let new_elbow = Point { x: ba.x, y: next[adjacent_idx].y };
                    if ep_idx == 0 {
                        next.insert(adjacent_idx + 1, new_elbow);
                    } else {
                        next.insert(adjacent_idx, new_elbow);
                    }
                }
            } else if let Some(ba) = before_adjacent.as_ref() {
                if ba.x != next[adjacent_idx].x && ba.y != next[adjacent_idx].y {
                    let new_elbow = Point { x: ba.x, y: next[adjacent_idx].y };
                    if ep_idx == 0 {
                        next.insert(adjacent_idx + 1, new_elbow);
                    } else {
                        next.insert(adjacent_idx, new_elbow);
                    }
                }
            }
        }
    }
    next
}

// ---------------------------------------------------------------------------
// offsetEndpointRoute
// ---------------------------------------------------------------------------

/// Port of JS `offsetEndpointRoute(route, endpointIndex, rect, side, rawOffset)`.
///
/// Moves endpoint `endpointIndex` to the offset position and rebuilds the route
/// via `route_with_points`. For spline routes the controls are preserved.
pub fn offset_endpoint_route(
    route: &RouteData,
    endpoint_index: usize,
    rect: &Rect,
    side: &str,
    raw_offset: f64,
) -> RouteData {
    let points = endpoint_offset_points(&route.points, endpoint_index, rect, side, raw_offset);
    let controls = if route.style == "spline" {
        route.controls.clone()
    } else {
        None
    };
    route_with_points(route, points, controls)
}

// ---------------------------------------------------------------------------
// offsetOrthogonalPolyline
// ---------------------------------------------------------------------------

/// Port of JS `offsetOrthogonalPolyline(points, delta)`.
///
/// Offsets an axis-aligned polyline perpendicularly to each segment by `delta`
/// (consistent winding), reconnecting the shifted right-angle corners.
/// Endpoints shift along their node surface; the mount stays on the same
/// surface at a parallel offset. Returns `points` unchanged if it has fewer
/// than 2 elements or is empty.
pub fn offset_orthogonal_polyline(points: &[Point], delta: f64) -> Vec<Point> {
    if points.len() < 2 {
        return points.to_vec();
    }
    struct Seg {
        a: Point,
        b: Point,
        vertical: bool,
    }
    // JS Math.sign returns 0 for 0, but f64::signum returns ±1 for ±0.
    // Use explicit zero-aware sign matching JS semantics.
    fn js_sign(v: f64) -> f64 {
        if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 }
    }
    let mut segments: Vec<Seg> = Vec::new();
    for index in 0..points.len() - 1 {
        let a = &points[index];
        let b = &points[index + 1];
        let dir_x = js_sign(b.x - a.x);
        let dir_y = js_sign(b.y - a.y);
        // normal = rotate dir 90° CCW: (dirY, -dirX)
        let normal_x = dir_y;
        let normal_y = -dir_x;
        segments.push(Seg {
            a: Point {
                x: a.x + normal_x * delta,
                y: a.y + normal_y * delta,
            },
            b: Point {
                x: b.x + normal_x * delta,
                y: b.y + normal_y * delta,
            },
            vertical: a.x == b.x,
        });
    }
    // Build output: start with the shifted first point, then reconnect corners.
    let mut out = vec![segments[0].a.clone()];
    for index in 0..segments.len() - 1 {
        let current = &segments[index];
        let next = &segments[index + 1];
        // Corner: x comes from the vertical segment, y from the horizontal.
        let x = if current.vertical { current.a.x } else { next.a.x };
        let y = if current.vertical { next.a.y } else { current.a.y };
        out.push(Point { x, y });
    }
    out.push(segments[segments.len() - 1].b.clone());
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Rect};
    use indexmap::IndexMap;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    /// Minimal RouteData for testing (orthogonal style with integer coordinates
    /// so the sample-bounds / d comparisons are simple).
    fn orthogonal_route(points: Vec<Point>) -> RouteData {
        let samples = crate::route_geometry::line_samples(&points);
        let sb = bounds_for_points(&{
            let mut v = points.clone();
            v.extend(samples.iter().cloned());
            v
        });
        let bends = bend_count(&points);
        let label = samples.get(samples.len() / 2).cloned()
            .or_else(|| points.get(points.len() / 2).cloned())
            .unwrap_or(pt(0.0, 0.0));
        RouteData {
            d: build_ml_path(&points),
            points,
            controls: None,
            samples,
            sample_bounds: sb,
            bends,
            label_x: label.x,
            label_y: label.y,
            style: "orthogonal".into(),
            extra: IndexMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // endpointSide
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_side_all_four() {
        // Node: rect={x:10,y:20,w:80,h:40}
        // left={x:10,y:40}→"left", right={x:90,y:40}→"right"
        // top={x:50,y:20}→"top", bottom={x:50,y:60}→"bottom"
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_side(&r, &pt(10.0, 40.0)), "left");
        assert_eq!(endpoint_side(&r, &pt(90.0, 40.0)), "right");
        assert_eq!(endpoint_side(&r, &pt(50.0, 20.0)), "top");
        assert_eq!(endpoint_side(&r, &pt(50.0, 60.0)), "bottom");
    }

    #[test]
    fn endpoint_side_interior_returns_empty() {
        // Node: interior point {x:50,y:40} → ""
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_side(&r, &pt(50.0, 40.0)), "");
    }

    #[test]
    fn endpoint_side_corner_prefers_left_over_top() {
        // Top-left corner: x==rect.x is checked first → "left"
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_side(&r, &pt(10.0, 20.0)), "left");
    }

    // -----------------------------------------------------------------------
    // sideNeedsPostSelectionCentering
    // -----------------------------------------------------------------------

    #[test]
    fn side_centering_all_four_sides_true() {
        // Node: left/right/top/bottom → true; "" or "other" → false
        assert!(side_needs_post_selection_centering("left"));
        assert!(side_needs_post_selection_centering("right"));
        assert!(side_needs_post_selection_centering("top"));
        assert!(side_needs_post_selection_centering("bottom"));
        assert!(!side_needs_post_selection_centering(""));
        assert!(!side_needs_post_selection_centering("diagonal"));
    }

    // -----------------------------------------------------------------------
    // axisAlignedSegments
    // -----------------------------------------------------------------------

    #[test]
    fn axis_aligned_segments_l_shape() {
        // Node: points [{x:0,y:0},{x:100,y:0},{x:100,y:50}]
        // → [{orientation:"horizontal",line:0,min:0,max:100},
        //    {orientation:"vertical",line:100,min:0,max:50}]
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)]);
        let segs = axis_aligned_segments(&route);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 0.0);
        assert_eq!(segs[0].max, 100.0);
        assert_eq!(segs[1].orientation, "vertical");
        assert_eq!(segs[1].line, 100.0);
        assert_eq!(segs[1].min, 0.0);
        assert_eq!(segs[1].max, 50.0);
    }

    #[test]
    fn axis_aligned_segments_single_segment() {
        // Node: [{x:50,y:0},{x:150,y:0}] → [{orientation:"horizontal",line:0,min:50,max:150}]
        let route = orthogonal_route(vec![pt(50.0, 0.0), pt(150.0, 0.0)]);
        let segs = axis_aligned_segments(&route);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 50.0);
        assert_eq!(segs[0].max, 150.0);
    }

    #[test]
    fn axis_aligned_segments_empty_route() {
        let route = orthogonal_route(vec![]);
        assert_eq!(axis_aligned_segments(&route).len(), 0);
    }

    // -----------------------------------------------------------------------
    // sharedSegmentLength
    // -----------------------------------------------------------------------

    #[test]
    fn shared_segment_length_horizontal_overlap() {
        // Node: left=[horiz,y=0,min=0,max=100], right=[horiz,y=0,min=50,max=150]
        // overlap = min(100,150)-max(0,50) = 100-50 = 50
        let left = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 100.0 };
        let right = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 50.0, max: 150.0 };
        assert_eq!(shared_segment_length(&left, &right), 50.0);
    }

    #[test]
    fn shared_segment_length_different_orientation() {
        // Node: vertical vs horizontal → 0
        let v = AxisAlignedSegment { orientation: "vertical", line: 100.0, min: 0.0, max: 50.0 };
        let h = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 100.0 };
        assert_eq!(shared_segment_length(&v, &h), 0.0);
    }

    #[test]
    fn shared_segment_length_different_line() {
        // Node: both horizontal but y=0 vs y=10 → 0
        let a = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 150.0 };
        let b = AxisAlignedSegment { orientation: "horizontal", line: 10.0, min: 0.0, max: 150.0 };
        assert_eq!(shared_segment_length(&a, &b), 0.0);
    }

    #[test]
    fn shared_segment_length_no_overlap() {
        // Node: [0,100] vs [200,300] → 0
        let a = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 100.0 };
        let b = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 200.0, max: 300.0 };
        assert_eq!(shared_segment_length(&a, &b), 0.0);
    }

    // -----------------------------------------------------------------------
    // routeCollidesWithNonEndpoints
    // -----------------------------------------------------------------------

    #[test]
    fn route_collides_with_non_endpoints_through_blocker() {
        // Node: route through C (x=100..150,y=0..50); from=A,to=B
        // collides → true
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        node_rects.insert("C".to_string(), rect(100.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        // Route going through C (x=100..150)
        let route = orthogonal_route(vec![pt(50.0, 25.0), pt(125.0, 25.0), pt(200.0, 25.0)]);
        assert!(route_collides_with_non_endpoints(&route, &rel, &input));
    }

    #[test]
    fn route_collides_avoids_below() {
        // Node: route going under C → false
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        node_rects.insert("C".to_string(), rect(100.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        // Route going below C
        let route = orthogonal_route(vec![
            pt(50.0, 25.0),
            pt(50.0, 75.0),
            pt(200.0, 75.0),
            pt(200.0, 25.0),
        ]);
        assert!(!route_collides_with_non_endpoints(&route, &rel, &input));
    }

    // -----------------------------------------------------------------------
    // routeHasEndpointTraversal
    // -----------------------------------------------------------------------

    #[test]
    fn route_has_endpoint_traversal_inside_from() {
        // Node: a sample point strictly inside A → true
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        let mut route = orthogonal_route(vec![pt(50.0, 25.0), pt(200.0, 25.0)]);
        // Override samples with one strictly inside A
        route.samples = vec![pt(25.0, 25.0)];
        assert!(route_has_endpoint_traversal(&route, &rel, &input));
    }

    #[test]
    fn route_has_no_endpoint_traversal() {
        // Node: sample outside both endpoints → false
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        let mut route = orthogonal_route(vec![pt(50.0, 25.0), pt(200.0, 25.0)]);
        route.samples = vec![pt(125.0, 75.0)];
        assert!(!route_has_endpoint_traversal(&route, &rel, &input));
    }

    // -----------------------------------------------------------------------
    // endpointSpreadOffset
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_spread_offset_left_side_three() {
        // Node: rect h=40, side=left, 3 edges
        // index=0 → (1/4-0.5)*40 = -10
        // index=1 → (2/4-0.5)*40 = 0
        // index=2 → (3/4-0.5)*40 = 10
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_spread_offset(0, 3, &r, "left"), -10.0);
        assert_eq!(endpoint_spread_offset(1, 3, &r, "left"), 0.0);
        assert_eq!(endpoint_spread_offset(2, 3, &r, "left"), 10.0);
    }

    #[test]
    fn endpoint_spread_offset_top_side_two() {
        // Node: rect w=80, side=top, 2 edges
        // index=0 → (1/3-0.5)*80 = -13.333...
        // index=1 → (2/3-0.5)*80 = 13.333...
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let v0 = endpoint_spread_offset(0, 2, &r, "top");
        let v1 = endpoint_spread_offset(1, 2, &r, "top");
        assert!((v0 - (-13.333_333_333_333_336)).abs() < 1e-9);
        assert!((v1 - 13.333_333_333_333_33).abs() < 1e-9);
    }

    #[test]
    fn endpoint_spread_offset_single_right() {
        // Node: 1 edge on right side (h=40) → (1/2-0.5)*40 = 0
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_spread_offset(0, 1, &r, "right"), 0.0);
    }

    // -----------------------------------------------------------------------
    // endpointOffsetPoints (private, tested through offsetEndpointRoute)
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_offset_points_2pt_left_offset10() {
        // Node: pts=[{x:100,y:240},{x:200,y:240}], ep=0, rect={x:100,y:200,w:60,h:80},
        // side="left", rawOffset=10
        // → [{x:100,y:250},{x:82,y:250},{x:82,y:240},{x:200,y:240}]
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let pts = vec![pt(100.0, 240.0), pt(200.0, 240.0)];
        let result = endpoint_offset_points(&pts, 0, &r, "left", 10.0);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], pt(100.0, 250.0));
        assert_eq!(result[1], pt(82.0, 250.0));
        assert_eq!(result[2], pt(82.0, 240.0));
        assert_eq!(result[3], pt(200.0, 240.0));
    }

    #[test]
    fn endpoint_offset_points_2pt_left_offset0() {
        // Node: rawOffset=0 → anchor at center: rect.y+h/2=240
        // → [{x:100,y:240},{x:82,y:240},{x:82,y:240},{x:200,y:240}]
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let pts = vec![pt(100.0, 240.0), pt(200.0, 240.0)];
        let result = endpoint_offset_points(&pts, 0, &r, "left", 0.0);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], pt(100.0, 240.0));
        assert_eq!(result[1], pt(82.0, 240.0));
        assert_eq!(result[2], pt(82.0, 240.0));
        assert_eq!(result[3], pt(200.0, 240.0));
    }

    #[test]
    fn endpoint_offset_points_multipt_centering() {
        // Node: 4-pt route, centering (rawOffset=0)
        // pts=[{100,240},{82,240},{82,300},{200,300}], ep=0, left
        // → unchanged because anchor already at (100,240) and adjacentY=240
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let pts = vec![pt(100.0, 240.0), pt(82.0, 240.0), pt(82.0, 300.0), pt(200.0, 300.0)];
        let result = endpoint_offset_points(&pts, 0, &r, "left", 0.0);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], pt(100.0, 240.0));
        assert_eq!(result[1], pt(82.0, 240.0));
    }

    // -----------------------------------------------------------------------
    // offsetEndpointRoute
    // -----------------------------------------------------------------------

    #[test]
    fn offset_endpoint_route_2pt_left_10() {
        // Node: offset endpoint 0 on left by rawOffset=10 produces 4-pt route
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let route = orthogonal_route(vec![pt(100.0, 240.0), pt(200.0, 240.0)]);
        let result = offset_endpoint_route(&route, 0, &r, "left", 10.0);
        assert_eq!(result.points[0], pt(100.0, 250.0));
        assert_eq!(result.points.len(), 4);
        assert_eq!(result.style, "orthogonal");
    }

    // -----------------------------------------------------------------------
    // routeWithPoints
    // -----------------------------------------------------------------------

    #[test]
    fn route_with_points_orthogonal_l_shape() {
        // Node: 4-pt orthogonal route, already simplified
        // d = "M 100 240 L 82 240 L 82 300 L 200 300"
        // bends = 2
        // lineSamples: 10 steps × 3 segments = 30 samples; mid=floor(30/2)=15
        // Segment 0 steps: t=0.1..1.0 on [{100,240}→{82,240}]
        // Segment 1 steps: t=0.1..1.0 on [{82,240}→{82,300}]
        //   samples[10]={82,240}, [15]={82,270} … wait sample[15] is step 6 of seg 1:
        //   82+0*6*(1/10-0)=82, 240+(300-240)*6/10=276 → {82,276}
        // Node: confirmed in Node.js: label={x:82,y:276}
        let route = orthogonal_route(vec![pt(0.0, 0.0)]);
        let pts = vec![pt(100.0, 240.0), pt(82.0, 240.0), pt(82.0, 300.0), pt(200.0, 300.0)];
        let result = route_with_points(&route, pts, None);
        assert_eq!(result.d, "M 100 240 L 82 240 L 82 300 L 200 300");
        assert_eq!(result.bends, 2);
        assert_eq!(result.label_x, 82.0);
        assert_eq!(result.label_y, 276.0); // Node: samples[15]={x:82,y:276}
    }

    #[test]
    fn route_with_points_orthogonal_2pt_straight() {
        // Node: 2-pt horizontal → bends=0
        // lineSamples: 10 steps on [{10,40}→{200,40}]; mid=floor(10/2)=5
        // step 6 (index 5): t=0.6, x=10+(200-10)*0.6=10+114=124 → {124,40}
        // Node: confirmed label={x:124,y:40}
        let route = orthogonal_route(vec![pt(0.0, 0.0)]);
        let pts = vec![pt(10.0, 40.0), pt(200.0, 40.0)];
        let result = route_with_points(&route, pts, None);
        assert_eq!(result.d, "M 10 40 L 200 40");
        assert_eq!(result.bends, 0);
        assert_eq!(result.label_x, 124.0); // Node: samples[5]={x:124,y:40}
        assert_eq!(result.label_y, 40.0);
    }

    // -----------------------------------------------------------------------
    // offsetOrthogonalPolyline
    // -----------------------------------------------------------------------

    #[test]
    fn offset_orthogonal_polyline_l_shape_delta10() {
        // Node: pts=[{0,0},{100,0},{100,50}], delta=10
        // → [{0,-10},{110,-10},{110,50}]
        let pts = vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)];
        let result = offset_orthogonal_polyline(&pts, 10.0);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], pt(0.0, -10.0));
        assert_eq!(result[1], pt(110.0, -10.0));
        assert_eq!(result[2], pt(110.0, 50.0));
    }

    #[test]
    fn offset_orthogonal_polyline_horizontal_delta5() {
        // Node: pts=[{0,0},{100,0}], delta=5 → [{0,-5},{100,-5}]
        let pts = vec![pt(0.0, 0.0), pt(100.0, 0.0)];
        let result = offset_orthogonal_polyline(&pts, 5.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], pt(0.0, -5.0));
        assert_eq!(result[1], pt(100.0, -5.0));
    }

    #[test]
    fn offset_orthogonal_polyline_vertical_then_horiz_delta12() {
        // Node: pts=[{0,0},{0,50},{100,50}], delta=12
        // → [{12,0},{12,38},{100,38}]
        let pts = vec![pt(0.0, 0.0), pt(0.0, 50.0), pt(100.0, 50.0)];
        let result = offset_orthogonal_polyline(&pts, 12.0);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], pt(12.0, 0.0));
        assert_eq!(result[1], pt(12.0, 38.0));
        assert_eq!(result[2], pt(100.0, 38.0));
    }

    #[test]
    fn offset_orthogonal_polyline_null_returns_empty() {
        // Node: empty points → empty result
        let result = offset_orthogonal_polyline(&[], 10.0);
        assert!(result.is_empty());
    }

    #[test]
    fn offset_orthogonal_polyline_single_point_unchanged() {
        // Node: single point → [point] unchanged
        let pts = vec![pt(0.0, 0.0)];
        let result = offset_orthogonal_polyline(&pts, 10.0);
        assert_eq!(result, pts);
    }
}
