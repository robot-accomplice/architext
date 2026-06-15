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
use crate::route_constants::rect_center;
use crate::route_geometry::{
    bend_count, bounds_for_points, line_samples, sample_cubic, sample_line,
    segment_intersects_rect,
};
use crate::route_labels::{with_readable_label, RouteForLabel};
use crate::route_ports::{anchor_for, port_for, side_vector, surface_capacity, PORT_STUB};
use crate::route_rendering::{path_to_svg_with_hops, simplify_orthogonal_points, RouteRef};
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
// Pass C1 — route-construction / cleanup helpers (routeEdges.js ~L54–449)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// renderedAxisAlignedSegments (L66)
// ---------------------------------------------------------------------------

/// Port of JS `renderedAxisAlignedSegments(points)`.
///
/// Takes a raw `&[Point]` (not a `RouteData`) — the function in JS operates on
/// `route.points` but also on `otherRoute.points` directly.  Returns segments
/// using the same `AxisAlignedSegment { orientation, line, min, max }` struct
/// (where `line` is the constant coordinate, matching the `renderedAxisAlignedSegments`
/// convention: x for vertical, y for horizontal).
///
/// This is distinct from `axis_aligned_segments` which takes `&RouteData` and
/// was ported for other callers (same semantics, different entry signature).
pub fn rendered_axis_aligned_segments(points: &[Point]) -> Vec<AxisAlignedSegment> {
    let mut segments = Vec::new();
    let len = points.len();
    if len < 2 {
        return segments;
    }
    for index in 0..len - 1 {
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
// finalSharedSegmentStats (L80)
// ---------------------------------------------------------------------------

/// Return type for `final_shared_segment_stats`.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedSegmentStats {
    pub count: usize,
    pub length: f64,
}

/// Port of JS `finalSharedSegmentStats(route, allRoutes)`.
///
/// Returns the number of shared-segment pairings and their total overlap length
/// between `route` and every other route in `all_routes` (skipping the route
/// itself by pointer identity, modelled here by skipping the element at
/// `self_index`).  `self_index` is the index of `route` in `all_routes`.
///
/// Note: the JS version uses `otherRoute === route` reference equality to skip
/// self.  In Rust we cannot compare by pointer through `RouteData` refs in a
/// type-safe way without `std::ptr::eq`.  The caller must pass the correct
/// `self_index` (or `usize::MAX` to skip nothing).
pub fn final_shared_segment_stats(
    route: &RouteData,
    all_routes: &[RouteData],
    self_index: usize,
) -> SharedSegmentStats {
    let route_segments = rendered_axis_aligned_segments(&route.points);
    let mut count = 0usize;
    let mut length = 0.0f64;
    for (i, other_route) in all_routes.iter().enumerate() {
        if i == self_index {
            continue;
        }
        for left in &route_segments {
            for right in rendered_axis_aligned_segments(&other_route.points) {
                if left.orientation != right.orientation || left.line != right.line {
                    continue;
                }
                let overlap = f64::min(left.max, right.max) - f64::max(left.min, right.min);
                if overlap > 1.0 {
                    count += 1;
                    length += overlap;
                }
            }
        }
    }
    SharedSegmentStats { count, length }
}

// ---------------------------------------------------------------------------
// renderOrthogonalRoute (L54)
// ---------------------------------------------------------------------------

/// Output of `render_orthogonal_route` — the route with hop-aware SVG `d`,
/// recomputed `sample_bounds`, and shared-segment diagnostics stored in `extra`.
/// The `extra` map carries "sharedSegments" and "sharedSegmentLength" as JSON
/// numbers, faithfully mirroring the JS spread operator that adds those fields.
pub fn render_orthogonal_route(
    route: &RouteData,
    all_routes: &[RouteData],
    self_index: usize,
) -> RouteData {
    let stats = final_shared_segment_stats(route, all_routes, self_index);
    // Build RouteRef slice for path_to_svg_with_hops (excluding self).
    let refs: Vec<RouteRef<'_>> = all_routes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != self_index)
        .map(|(_, r)| RouteRef::WithPoints(&r.points))
        .collect();
    let d = path_to_svg_with_hops(&route.points, &refs);
    let sample_bounds = bounds_for_points(&route.samples);

    let mut next = route.clone();
    next.d = d;
    next.sample_bounds = sample_bounds;
    next.style = "orthogonal".to_string();
    next.extra.insert(
        "sharedSegments".to_string(),
        serde_json::Value::Number(serde_json::Number::from(stats.count)),
    );
    next.extra.insert(
        "sharedSegmentLength".to_string(),
        serde_json::json!(stats.length),
    );

    // Apply withReadableLabel to adjust label position on very short routes.
    let for_label = RouteForLabel {
        points: next.points.clone(),
        samples: next.samples.clone(),
        label_x: next.label_x,
        label_y: next.label_y,
    };
    let labeled = with_readable_label(&for_label);
    next.label_x = labeled.label_x;
    next.label_y = labeled.label_y;
    next
}

// ---------------------------------------------------------------------------
// sideEndpointKey (L108)
// ---------------------------------------------------------------------------

/// Port of JS `sideEndpointKey(nodeId, side)`.
///
/// Returns `"${nodeId} ${side}"` — a NUL-separated composite key matching
/// the JS template literal that uses the U+0000 byte as the separator between
/// `nodeId` and `side`.
pub fn side_endpoint_key(node_id: &str, side: &str) -> String {
    let mut key = String::with_capacity(node_id.len() + 1 + side.len());
    key.push_str(node_id);
    key.push('\0');
    key.push_str(side);
    key
}

// ---------------------------------------------------------------------------
// createEndpointSideUsage (L112)
// ---------------------------------------------------------------------------

/// Port of JS `createEndpointSideUsage()`.
///
/// The JS function returns an object with two closure methods sharing a `Map`.
/// In Rust we model it as a struct with two methods.
pub struct EndpointSideUsage<'a> {
    counts: IndexMap<String, u32>,
    /// Provides the `rect` lookup for the `isAvailable` check (nodeId → Rect).
    node_rects: &'a IndexMap<String, Rect>,
}

impl<'a> EndpointSideUsage<'a> {
    pub fn new(node_rects: &'a IndexMap<String, Rect>) -> Self {
        EndpointSideUsage {
            counts: IndexMap::new(),
            node_rects,
        }
    }

    /// Port of JS `isAvailable(nodeId, side, rect)`.
    ///
    /// Returns `true` when either the nodeId/side/rect are absent (falsy in JS)
    /// or the used count for this side is below `surfaceCapacity(rect, side)`.
    pub fn is_available(&self, node_id: Option<&str>, side: Option<&str>) -> bool {
        let (nid, sd) = match (node_id, side) {
            (Some(n), Some(s)) if !n.is_empty() && !s.is_empty() => (n, s),
            _ => return true,
        };
        let rect = match self.node_rects.get(nid) {
            Some(r) => r,
            None => return true,
        };
        let used = self.counts.get(&side_endpoint_key(nid, sd)).copied().unwrap_or(0);
        used < surface_capacity(rect, sd)
    }

    /// Port of JS `mark(nodeId, side)`.
    pub fn mark(&mut self, node_id: Option<&str>, side: Option<&str>) {
        let (nid, sd) = match (node_id, side) {
            (Some(n), Some(s)) if !n.is_empty() && !s.is_empty() => (n, s),
            _ => return,
        };
        let key = side_endpoint_key(nid, sd);
        let entry = self.counts.entry(key).or_insert(0);
        *entry += 1;
    }
}

// ---------------------------------------------------------------------------
// recenteredEndpointPoints (L131)
// ---------------------------------------------------------------------------

/// Port of JS `recenteredEndpointPoints(points, endpointIndex, rect, side)`.
///
/// Delegates to `endpoint_offset_points` with `rawOffset = 0`.
pub fn recentered_endpoint_points(
    points: &[Point],
    endpoint_index: usize,
    rect: &Rect,
    side: &str,
) -> Vec<Point> {
    endpoint_offset_points(points, endpoint_index, rect, side, 0.0)
}

// ---------------------------------------------------------------------------
// recenteredEndpointRoute (L229)
// ---------------------------------------------------------------------------

/// Port of JS `recenteredEndpointRoute(route, endpointIndex, rect, side)`.
pub fn recentered_endpoint_route(
    route: &RouteData,
    endpoint_index: usize,
    rect: &Rect,
    side: &str,
) -> RouteData {
    let points = recentered_endpoint_points(&route.points, endpoint_index, rect, side);
    let controls = if route.style == "spline" { route.controls.clone() } else { None };
    route_with_points(route, points, controls)
}

// ---------------------------------------------------------------------------
// Input types for C1 functions that need richer context
// ---------------------------------------------------------------------------

/// Richer relationship needed by C1 cleanup functions.
///
/// `preferred_start_side` / `preferred_end_side` mirror the JS fields used by
/// `alignedFixedPortRoute`.  The `style` field mirrors `route.style` for
/// `enforceEndpointStubs` (which skips spline/straight routes via the
/// relationship).
pub struct RelationshipC1<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub preferred_start_side: Option<&'a str>,
    pub preferred_end_side: Option<&'a str>,
}

/// Richer input for C1 functions that need `fixedPorts` from node rects.
///
/// `fixed_ports` is an optional map from nodeId → fixedPorts flag.  When
/// absent (`None`) all rects are treated as not-fixed (matching the JS
/// `rect?.fixedPorts` falsy path).
pub struct RouteInputC1<'a> {
    pub visible_node_ids: &'a [String],
    pub node_rects: &'a IndexMap<String, Rect>,
    pub fixed_ports: Option<&'a IndexMap<String, bool>>,
}

impl<'a> RouteInputC1<'a> {
    fn is_fixed_ports(&self, node_id: &str) -> bool {
        self.fixed_ports
            .and_then(|m| m.get(node_id).copied())
            .unwrap_or(false)
    }

    fn as_route_input(&self) -> RouteInput<'a> {
        RouteInput {
            visible_node_ids: self.visible_node_ids,
            node_rects: self.node_rects,
        }
    }
}

// ---------------------------------------------------------------------------
// collapseAlignedOpposingSurfaceRoute (L241)
// ---------------------------------------------------------------------------

/// Port of JS `collapseAlignedOpposingSurfaceRoute(route, firstSide, lastSide, relationship, input)`.
///
/// If both endpoints are on opposing H/V sides and share the same y/x coordinate,
/// tries to collapse the route to a 2-point straight line (rejecting if it
/// produces a non-endpoint collision).
pub fn collapse_aligned_opposing_surface_route(
    route: &RouteData,
    first_side: &str,
    last_side: &str,
    relationship: &RelationshipC1<'_>,
    input: &RouteInputC1<'_>,
) -> RouteData {
    if route.points.is_empty() {
        return route.clone();
    }
    let first = route.points[0].clone();
    let last = route.points[route.points.len() - 1].clone();
    let simple_rel = Relationship { from: relationship.from, to: relationship.to };
    let route_input = input.as_route_input();

    if (first_side == "left" || first_side == "right")
        && (last_side == "left" || last_side == "right")
        && first.y == last.y
    {
        let collapsed = route_with_points(route, vec![first, last], route.controls.clone());
        if route_collides_with_non_endpoints(&collapsed, &simple_rel, &route_input) {
            return route.clone();
        }
        return collapsed;
    }
    if (first_side == "top" || first_side == "bottom")
        && (last_side == "top" || last_side == "bottom")
        && first.x == last.x
    {
        let collapsed = route_with_points(route, vec![first, last], route.controls.clone());
        if route_collides_with_non_endpoints(&collapsed, &simple_rel, &route_input) {
            return route.clone();
        }
        return collapsed;
    }
    route.clone()
}

// ---------------------------------------------------------------------------
// alignedFixedPortRoute (L255)
// ---------------------------------------------------------------------------

/// Port of JS `alignedFixedPortRoute(route, relationship, input)`.
///
/// When either endpoint node has `fixedPorts` set and the relationship has a
/// `preferredStartSide` / `preferredEndSide`, aligns the adjacent point so the
/// stub is axis-perpendicular (matching the port direction).
pub fn aligned_fixed_port_route(
    route: &RouteData,
    relationship: &RelationshipC1<'_>,
    input: &RouteInputC1<'_>,
) -> RouteData {
    if route.points.is_empty() {
        return route.clone();
    }
    let from_rect = input.node_rects.get(relationship.from);
    let to_rect = input.node_rects.get(relationship.to);
    let from_fixed = from_rect.is_some() && input.is_fixed_ports(relationship.from);
    let to_fixed = to_rect.is_some() && input.is_fixed_ports(relationship.to);

    let mut points = route.points.clone();
    let mut modified = false;

    if let (true, Some(pss), Some(_)) = (from_fixed, relationship.preferred_start_side, points.get(1)) {
        if pss == "left" || pss == "right" {
            points[1].y = points[0].y;
        } else {
            points[1].x = points[0].x;
        }
        modified = true;
    }
    if points.len() > 1 {
        if let (true, Some(pes)) = (to_fixed, relationship.preferred_end_side) {
            let before_end = points.len() - 2;
            let end = points.len() - 1;
            if pes == "left" || pes == "right" {
                points[before_end].y = points[end].y;
            } else {
                points[before_end].x = points[end].x;
            }
            modified = true;
        }
    }

    if modified {
        route_with_points(route, points, route.controls.clone())
    } else {
        route.clone()
    }
}

// ---------------------------------------------------------------------------
// sharedSegmentCount (L312)
// ---------------------------------------------------------------------------

/// Port of JS `sharedSegmentCount(route, otherRoutes)`.
///
/// Counts the number of segment-pair overlaps (by `sharedSegmentLength > 1`)
/// between `route` and every route in `other_routes`.
pub fn shared_segment_count(route: &RouteData, other_routes: &[RouteData]) -> usize {
    let mut count = 0usize;
    for segment in axis_aligned_segments(route) {
        for other in other_routes {
            for other_seg in axis_aligned_segments(other) {
                if shared_segment_length(&segment, &other_seg) > 1.0 {
                    count += 1;
                }
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// nonEndpointNodeCollisionCount (L339)
// ---------------------------------------------------------------------------

/// Port of JS `nonEndpointNodeCollisionCount(route, relationship, input)`.
///
/// Counts visible nodes (excluding `relationship.from` and `.to`) whose rect
/// is intersected by `route` (using `routeIntersectsRect(route, rect, 0)`).
pub fn non_endpoint_node_collision_count(
    route: &RouteData,
    relationship: &Relationship<'_>,
    input: &RouteInput<'_>,
) -> usize {
    let mut count = 0usize;
    for node_id in input.visible_node_ids {
        if node_id == relationship.from || node_id == relationship.to {
            continue;
        }
        if let Some(rect) = input.node_rects.get(node_id.as_str()) {
            if route_intersects_rect(route, rect, 0.0) {
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// recenteredWithoutNewSharedSegments (L324)
// ---------------------------------------------------------------------------

/// Port of JS `recenteredWithoutNewSharedSegments(route, nextRoute, otherRoutes, relationship, input)`.
///
/// Accepts `next_route` only when it does not increase either non-endpoint
/// collisions or shared-segment count vs. the current `route`.
/// When `relationship` and `input` are `None`, collision count is skipped (== 0).
pub fn recentered_without_new_shared_segments(
    route: &RouteData,
    next_route: RouteData,
    other_routes: &[RouteData],
    relationship: Option<(&Relationship<'_>, &RouteInput<'_>)>,
) -> RouteData {
    let (next_col, cur_col) = match relationship {
        Some((rel, inp)) => (
            non_endpoint_node_collision_count(&next_route, rel, inp),
            non_endpoint_node_collision_count(route, rel, inp),
        ),
        None => (0, 0),
    };
    if next_col <= cur_col
        && shared_segment_count(&next_route, other_routes)
            <= shared_segment_count(route, other_routes)
    {
        next_route
    } else {
        route.clone()
    }
}

// ---------------------------------------------------------------------------
// routeWithFewestSharedSegments (L332)
// ---------------------------------------------------------------------------

/// Port of JS `routeWithFewestSharedSegments(routes, otherRoutes)`.
///
/// Picks the route from `routes` that has the fewest shared segments with
/// `other_routes`; breaks ties by `bends`.  `routes` may contain `None`
/// entries (mirroring the JS `.filter(Boolean)` step).
pub fn route_with_fewest_shared_segments<'a>(
    routes: &'a [Option<RouteData>],
    other_routes: &[RouteData],
) -> Option<&'a RouteData> {
    let mut best: Option<&RouteData> = None;
    let mut best_shared = usize::MAX;
    let mut best_bends = usize::MAX;
    for r in routes.iter().flatten() {
        let shared = shared_segment_count(r, other_routes);
        if shared < best_shared || (shared == best_shared && r.bends < best_bends) {
            best = Some(r);
            best_shared = shared;
            best_bends = r.bends;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// routeWithBestCleanupCandidate (L350)
// ---------------------------------------------------------------------------

/// Port of JS `routeWithBestCleanupCandidate(routes, otherRoutes, relationship, input)`.
///
/// Sorts by (nonEndpointCollisions ASC, sharedSegments ASC, bends ASC).
pub fn route_with_best_cleanup_candidate<'a>(
    routes: &'a [Option<RouteData>],
    other_routes: &[RouteData],
    relationship: &Relationship<'_>,
    input: &RouteInput<'_>,
) -> Option<&'a RouteData> {
    let mut best: Option<&RouteData> = None;
    let mut best_col = usize::MAX;
    let mut best_shared = usize::MAX;
    let mut best_bends = usize::MAX;
    for r in routes.iter().flatten() {
        let col = non_endpoint_node_collision_count(r, relationship, input);
        let shared = shared_segment_count(r, other_routes);
        let bends = r.bends;
        let better = col < best_col
            || (col == best_col && shared < best_shared)
            || (col == best_col && shared == best_shared && bends < best_bends);
        if better {
            best = Some(r);
            best_col = col;
            best_shared = shared;
            best_bends = bends;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// alignedFacingEndpointRoute (L358)
// ---------------------------------------------------------------------------

/// Port of JS `alignedFacingEndpointRoute(route, relationship, input, endpointGroups)`.
///
/// When the two endpoints directly face each other and the coordinate delta is
/// small, aligns the sparser-side endpoint to the busier side so the route
/// collapses to a straight 2-point line.
///
/// `endpoint_groups` maps `sideEndpointKey(nodeId, side)` → list of
/// relationship IDs that share that side (same shape as the JS `Map<string, string[]>`
/// built by the spread-shared-side pass).
pub fn aligned_facing_endpoint_route(
    route: &RouteData,
    relationship: &RelationshipC1<'_>,
    input: &RouteInputC1<'_>,
    endpoint_groups: &IndexMap<String, Vec<String>>,
) -> RouteData {
    if route.points.len() < 2 {
        return route.clone();
    }
    let from_rect = match input.node_rects.get(relationship.from) {
        Some(r) => r,
        None => return route.clone(),
    };
    let to_rect = match input.node_rects.get(relationship.to) {
        Some(r) => r,
        None => return route.clone(),
    };
    if input.is_fixed_ports(relationship.from) || input.is_fixed_ports(relationship.to) {
        return route.clone();
    }

    let start_side = endpoint_side(from_rect, &route.points[0]);
    let end_side = endpoint_side(to_rect, &route.points[route.points.len() - 1]);

    let horizontal_facing = (start_side == "right" && end_side == "left" && from_rect.x < to_rect.x)
        || (start_side == "left" && end_side == "right" && from_rect.x > to_rect.x);
    let vertical_facing = (start_side == "bottom" && end_side == "top" && from_rect.y < to_rect.y)
        || (start_side == "top" && end_side == "bottom" && from_rect.y > to_rect.y);

    if !horizontal_facing && !vertical_facing {
        return route.clone();
    }

    let source_count = endpoint_groups
        .get(&side_endpoint_key(relationship.from, start_side))
        .map(|v| v.len())
        .unwrap_or(1);
    let target_count = endpoint_groups
        .get(&side_endpoint_key(relationship.to, end_side))
        .map(|v| v.len())
        .unwrap_or(1);

    let last = &route.points[route.points.len() - 1];
    let first = &route.points[0];
    let coordinate_delta = if horizontal_facing {
        (first.y - last.y).abs()
    } else {
        (first.x - last.x).abs()
    };

    // JS: coordinateDelta < 1 || coordinateDelta > PORT_STUB — faithful two-bound check.
    #[allow(clippy::manual_range_contains)]
    if coordinate_delta < 1.0 || coordinate_delta > PORT_STUB {
        return route.clone();
    }

    let align_source = source_count <= target_count;
    let endpoint_index = if align_source { 0 } else { route.points.len() - 1 };
    let rect = if align_source { from_rect } else { to_rect };
    let side = if align_source { start_side } else { end_side };
    let anchor = if align_source {
        route.points[route.points.len() - 1].clone()
    } else {
        route.points[0].clone()
    };
    let center = rect_center(rect);
    let raw_offset = if horizontal_facing {
        anchor.y - center.y
    } else {
        anchor.x - center.x
    };

    let next_route = offset_endpoint_route(route, endpoint_index, rect, side, raw_offset);
    let first_pt = next_route.points[0].clone();
    let last_pt = next_route.points[next_route.points.len() - 1].clone();
    let next_controls = next_route.controls.clone();
    route_with_points(&next_route, vec![first_pt, last_pt], next_controls)
}

// ---------------------------------------------------------------------------
// endpointStubRoute (L398)
// ---------------------------------------------------------------------------

/// Port of JS `endpointStubRoute(route, relationship, input, endpointIndex)`.
///
/// Ensures the stub exiting the endpoint node is at least `PORT_STUB` long;
/// extends it if shorter.  No-ops for routes with < 3 points or when the
/// endpoint node rect is unknown.
pub fn endpoint_stub_route(
    route: &RouteData,
    from: &str,
    to: &str,
    node_rects: &IndexMap<String, Rect>,
    endpoint_index: usize,
) -> RouteData {
    if route.points.len() < 3 {
        return route.clone();
    }
    let node_id = if endpoint_index == 0 { from } else { to };
    let rect = match node_rects.get(node_id) {
        Some(r) => r,
        None => return route.clone(),
    };
    let mut points = route.points.clone();
    let anchor_idx = if endpoint_index == 0 { 0 } else { points.len() - 1 };
    let adjacent_idx = if endpoint_index == 0 { 1 } else { points.len() - 2 };
    let elbow_idx = if endpoint_index == 0 { 2 } else { points.len() - 3 };

    let anchor = points[anchor_idx].clone();
    let side = endpoint_side(rect, &anchor);
    if side.is_empty() {
        return route.clone();
    }

    let adj = points[adjacent_idx].clone();
    let dx = anchor.x - adj.x;
    let dy = anchor.y - adj.y;
    let current_stub_length = crate::js_compat::js_hypot(dx, dy);
    if current_stub_length >= PORT_STUB {
        return route.clone();
    }

    let vec = side_vector(side);
    let next_adjacent = Point {
        x: anchor.x + vec.x * PORT_STUB,
        y: anchor.y + vec.y * PORT_STUB,
    };
    let old_adjacent = points[adjacent_idx].clone();
    points[adjacent_idx] = next_adjacent.clone();

    if let Some(elbow) = points.get_mut(elbow_idx) {
        if (side == "left" || side == "right") && elbow.x == old_adjacent.x {
            elbow.x = next_adjacent.x;
        }
        if (side == "top" || side == "bottom") && elbow.y == old_adjacent.y {
            elbow.y = next_adjacent.y;
        }
    }

    route_with_points(route, points, route.controls.clone())
}

// ---------------------------------------------------------------------------
// routeWithEndpointStubs (L443)
// ---------------------------------------------------------------------------

/// Port of JS `routeWithEndpointStubs(route, relationship, input)`.
///
/// Applies `endpointStubRoute` to both the start endpoint (index 0) and the
/// end endpoint (last index), in sequence.
pub fn route_with_endpoint_stubs(
    route: &RouteData,
    from: &str,
    to: &str,
    node_rects: &IndexMap<String, Rect>,
) -> RouteData {
    let next = endpoint_stub_route(route, from, to, node_rects, 0);
    let last_idx = next.points.len().saturating_sub(1);
    endpoint_stub_route(&next, from, to, node_rects, last_idx)
}

// ---------------------------------------------------------------------------
// enforceEndpointStubs (L432)
// ---------------------------------------------------------------------------

/// Minimal relationship descriptor needed by `enforce_endpoint_stubs`.
pub struct PlanRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub style: Option<String>,
}

/// Port of JS `enforceEndpointStubs(plannedRawRoutes, input)`.
///
/// Applies `routeWithEndpointStubs` to every non-spline, non-straight route in
/// `planned_raw_routes` (a parallel `(id, route)` list).
pub fn enforce_endpoint_stubs(
    planned_raw_routes: Vec<(String, RouteData)>,
    relationships: &[PlanRelationship],
    node_rects: &IndexMap<String, Rect>,
) -> Vec<(String, RouteData)> {
    let rel_by_id: IndexMap<&str, &PlanRelationship> =
        relationships.iter().map(|r| (r.id.as_str(), r)).collect();

    planned_raw_routes
        .into_iter()
        .map(|(rel_id, route)| {
            let rel = match rel_by_id.get(rel_id.as_str()) {
                Some(r) => r,
                None => return (rel_id, route),
            };
            let style = rel.style.as_deref().unwrap_or("orthogonal");
            if style == "spline" || style == "straight" {
                return (rel_id, route);
            }
            let next = route_with_endpoint_stubs(&route, &rel.from, &rel.to, node_rects);
            (rel_id, next)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Pass C2 — parallel-route separation subsystem (routeEdges.js ~L449–890)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// RouteSegment — axis-aligned segment with route index (for separation passes)
// ---------------------------------------------------------------------------

/// Axis-aligned segment that also carries the point `index` within its route.
///
/// `axisAlignedRouteSegments` in JS emits `{ route, index, orientation, line, min, max }`.
/// In Rust we carry `index` but not the route reference (callers hold the route separately).
#[derive(Debug, Clone, PartialEq)]
pub struct RouteSegment {
    /// Index of the *first* point of this segment within `route.points`.
    pub index: usize,
    /// "horizontal" or "vertical"
    pub orientation: &'static str,
    /// Constant coordinate (x for vertical segments, y for horizontal).
    pub line: f64,
    pub min: f64,
    pub max: f64,
}

// ---------------------------------------------------------------------------
// axisAlignedRouteSegments (L449)
// ---------------------------------------------------------------------------

/// Port of JS `axisAlignedRouteSegments(route)`.
///
/// Like `axis_aligned_segments` but also records the segment's point `index`
/// within `route.points`.  That index is used by the shifted-segment helpers.
pub fn axis_aligned_route_segments(route: &RouteData) -> Vec<RouteSegment> {
    let mut segments = Vec::new();
    let points = &route.points;
    for index in 0..points.len().saturating_sub(1) {
        let start = &points[index];
        let end = &points[index + 1];
        if start.x == end.x {
            segments.push(RouteSegment {
                index,
                orientation: "vertical",
                line: start.x,
                min: f64::min(start.y, end.y),
                max: f64::max(start.y, end.y),
            });
        } else if start.y == end.y {
            segments.push(RouteSegment {
                index,
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
// CloseParallelPair — return type of closeParallelSegmentPair
// ---------------------------------------------------------------------------

/// Returned by `close_parallel_segment_pair`.
#[derive(Debug, Clone)]
pub struct CloseParallelPair {
    pub left_id: String,
    pub right_id: String,
    pub left: RouteSegment,
    pub right: RouteSegment,
}

// ---------------------------------------------------------------------------
// closeParallelSegmentPair (L477)
// ---------------------------------------------------------------------------

/// Port of JS `closeParallelSegmentPair(routeById)`.
///
/// Finds the first pair of routes that have either an exact shared segment
/// (`distance == 0 && overlap > 1`) or a close-parallel segment
/// (`overlap >= 72 && distance > 0 && distance <= 10`).
///
/// `route_by_id` is a slice of `(id, route)` pairs in insertion order
/// (mirrors the JS `[...routeById]` spread, which preserves Map insertion order).
pub fn close_parallel_segment_pair(route_by_id: &[(String, RouteData)]) -> Option<CloseParallelPair> {
    for left_route_index in 0..route_by_id.len() {
        for right_route_index in left_route_index + 1..route_by_id.len() {
            let (left_id, left_route) = &route_by_id[left_route_index];
            let (right_id, right_route) = &route_by_id[right_route_index];
            for left in axis_aligned_route_segments(left_route) {
                for right in axis_aligned_route_segments(right_route) {
                    if left.orientation != right.orientation {
                        continue;
                    }
                    let overlap = f64::min(left.max, right.max) - f64::max(left.min, right.min);
                    let distance = (left.line - right.line).abs();
                    let exact_shared = distance == 0.0 && overlap > 1.0;
                    let close_parallel = overlap >= 72.0 && distance > 0.0 && distance <= 10.0;
                    if exact_shared || close_parallel {
                        return Some(CloseParallelPair {
                            left_id: left_id.clone(),
                            right_id: right_id.clone(),
                            left,
                            right,
                        });
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// shiftedInternalSegmentRoute (L500)
// ---------------------------------------------------------------------------

/// Port of JS `shiftedInternalSegmentRoute(route, segment, delta)`.
///
/// Shifts an interior segment by `delta` along its perpendicular axis.
/// Returns `None` when the segment is at index 0 or at the last edge
/// (same guard as `segment.index <= 0 || segment.index >= route.points.length - 2`).
pub fn shifted_internal_segment_route(
    route: &RouteData,
    segment: &RouteSegment,
    delta: f64,
) -> Option<RouteData> {
    if segment.index == 0 || segment.index >= route.points.len().saturating_sub(2) {
        return None;
    }
    let mut points: Vec<Point> = route.points.to_vec();
    if segment.orientation == "vertical" {
        points[segment.index].x += delta;
        points[segment.index + 1].x += delta;
    } else {
        points[segment.index].y += delta;
        points[segment.index + 1].y += delta;
    }
    Some(route_with_points(route, points, route.controls.clone()))
}

// ---------------------------------------------------------------------------
// shiftedEndpointSegmentRoute (L513)
// ---------------------------------------------------------------------------

/// Port of JS `shiftedEndpointSegmentRoute(route, relationship, input, segment, delta)`.
///
/// Shifts an endpoint-adjacent segment by moving the endpoint's offset on its
/// node surface.  Returns `None` when the segment is not endpoint-adjacent, the
/// node has `fixedPorts`, or the side/offset combination is not applicable.
pub fn shifted_endpoint_segment_route(
    route: &RouteData,
    from: &str,
    to: &str,
    node_rects: &IndexMap<String, Rect>,
    fixed_ports: &IndexMap<String, bool>,
    segment: &RouteSegment,
    delta: f64,
) -> Option<RouteData> {
    let shifts_source_side = segment.index <= 1;
    let shifts_target_side = segment.index >= route.points.len().saturating_sub(3);
    let endpoint_index: Option<usize> = if shifts_source_side {
        Some(0)
    } else if shifts_target_side {
        Some(route.points.len() - 1)
    } else {
        None
    };
    let endpoint_index = endpoint_index?;
    let node_id = if shifts_source_side { from } else { to };
    let rect = node_rects.get(node_id)?;
    if fixed_ports.get(node_id).copied().unwrap_or(false) {
        return None;
    }
    let point = &route.points[endpoint_index];
    let side = endpoint_side(rect, point);
    if side.is_empty() {
        return None;
    }
    let center = rect_center(rect);
    let next_line = segment.line + delta;
    let raw_offset: Option<f64> = if segment.orientation == "horizontal"
        && (side == "left" || side == "right")
    {
        Some(next_line - center.y)
    } else if segment.orientation == "vertical" && (side == "top" || side == "bottom") {
        Some(next_line - center.x)
    } else {
        None
    };
    let raw_offset = raw_offset?;
    Some(offset_endpoint_route(route, endpoint_index, rect, side, raw_offset))
}

// ---------------------------------------------------------------------------
// shiftedDirectEndpointRoute (L535)
// ---------------------------------------------------------------------------

/// Port of JS `shiftedDirectEndpointRoute(route, relationship, input, segment, delta)`.
///
/// For 2-point routes (direct connections), shifts both endpoint mounts to
/// land on `segment.line + delta`.  Returns `None` when the route is not 2
/// points, the segment is not index 0, either rect is absent or fixedPorts, or
/// the sides do not match the segment orientation.
pub fn shifted_direct_endpoint_route(
    route: &RouteData,
    from: &str,
    to: &str,
    node_rects: &IndexMap<String, Rect>,
    fixed_ports: &IndexMap<String, bool>,
    segment: &RouteSegment,
    delta: f64,
) -> Option<RouteData> {
    if route.points.len() != 2 || segment.index != 0 {
        return None;
    }
    let from_rect = node_rects.get(from)?;
    let to_rect = node_rects.get(to)?;
    if fixed_ports.get(from).copied().unwrap_or(false)
        || fixed_ports.get(to).copied().unwrap_or(false)
    {
        return None;
    }
    let start_side = endpoint_side(from_rect, &route.points[0]);
    let end_side = endpoint_side(to_rect, &route.points[1]);
    let next_line = segment.line + delta;
    if segment.orientation == "vertical"
        && (start_side == "top" || start_side == "bottom")
        && (end_side == "top" || end_side == "bottom")
    {
        let source_offset = next_line - rect_center(from_rect).x;
        let target_offset = next_line - rect_center(to_rect).x;
        let intermediate =
            offset_endpoint_route(route, 0, from_rect, start_side, source_offset);
        return Some(offset_endpoint_route(
            &intermediate,
            1,
            to_rect,
            end_side,
            target_offset,
        ));
    }
    if segment.orientation == "horizontal"
        && (start_side == "left" || start_side == "right")
        && (end_side == "left" || end_side == "right")
    {
        let source_offset = next_line - rect_center(from_rect).y;
        let target_offset = next_line - rect_center(to_rect).y;
        let intermediate =
            offset_endpoint_route(route, 0, from_rect, start_side, source_offset);
        return Some(offset_endpoint_route(
            &intermediate,
            1,
            to_rect,
            end_side,
            target_offset,
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// routePairIndex (L568)
// ---------------------------------------------------------------------------

/// Port of JS `routePairIndex(relationship, relationships)`.
///
/// Returns the 0-based index of `relationship` within the sub-sequence of
/// relationships that share the same unordered `{from, to}` pair.
///
/// Iteration order is the same as `relationships` (JS Array iteration order).
pub fn route_pair_index(rel_id: &str, from: &str, to: &str, relationships: &[SeparationRelationship]) -> usize {
    let pair_key = {
        let mut parts = [from, to];
        parts.sort_unstable();
        format!("{}<->{}", parts[0], parts[1])
    };
    let mut pair_index = 0usize;
    for candidate in relationships {
        if candidate.id == rel_id {
            return pair_index;
        }
        let candidate_key = {
            let mut parts = [candidate.from.as_str(), candidate.to.as_str()];
            parts.sort_unstable();
            format!("{}<->{}", parts[0], parts[1])
        };
        if candidate_key == pair_key {
            pair_index += 1;
        }
    }
    pair_index
}

// ---------------------------------------------------------------------------
// routeEndpointsArePerpendicular (L623)
// ---------------------------------------------------------------------------

/// Port of JS `routeEndpointsArePerpendicular(route, relationship, input)`.
///
/// Returns `true` when the first and last segments of `route` exit their
/// respective endpoint nodes perpendicularly (i.e. horizontal exit from
/// left/right sides, vertical exit from top/bottom sides).
pub fn route_endpoints_are_perpendicular(
    route: &RouteData,
    from: &str,
    to: &str,
    node_rects: &IndexMap<String, Rect>,
) -> bool {
    struct Ep<'a> {
        node_id: &'a str,
        point_index: usize,
        adjacent_index: usize,
    }
    let endpoints = [
        Ep { node_id: from, point_index: 0, adjacent_index: 1 },
        Ep {
            node_id: to,
            point_index: route.points.len().saturating_sub(1),
            adjacent_index: route.points.len().saturating_sub(2),
        },
    ];
    for ep in &endpoints {
        let rect = match node_rects.get(ep.node_id) {
            Some(r) => r,
            None => continue,
        };
        let point = match route.points.get(ep.point_index) {
            Some(p) => p,
            None => continue,
        };
        let adjacent = match route.points.get(ep.adjacent_index) {
            Some(p) => p,
            None => continue,
        };
        let side = endpoint_side(rect, point);
        if side.is_empty() {
            return false;
        }
        if (side == "left" || side == "right") && point.y != adjacent.y {
            return false;
        }
        if (side == "top" || side == "bottom") && point.x != adjacent.x {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// closeParallelRunCountForRoutes (L641)
// ---------------------------------------------------------------------------

/// Port of JS `closeParallelRunCountForRoutes(routeById)`.
///
/// Counts all close-parallel segment pairs across all route pairs (order-preserving
/// over `route_by_id` slice).
pub fn close_parallel_run_count_for_routes(route_by_id: &[(String, RouteData)]) -> usize {
    let mut count = 0usize;
    for left_route_index in 0..route_by_id.len() {
        for right_route_index in left_route_index + 1..route_by_id.len() {
            for left in axis_aligned_route_segments(&route_by_id[left_route_index].1) {
                for right in axis_aligned_route_segments(&route_by_id[right_route_index].1) {
                    if left.orientation != right.orientation {
                        continue;
                    }
                    let overlap = f64::min(left.max, right.max) - f64::max(left.min, right.min);
                    if overlap >= 72.0 && (left.line - right.line).abs() <= 10.0 {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// closeParallelRunCountBetween (L658)
// ---------------------------------------------------------------------------

/// Port of JS `closeParallelRunCountBetween(leftRoute, rightRoute)`.
pub fn close_parallel_run_count_between(left_route: &RouteData, right_route: &RouteData) -> usize {
    let mut count = 0usize;
    for left in axis_aligned_route_segments(left_route) {
        for right in axis_aligned_route_segments(right_route) {
            if left.orientation != right.orientation {
                continue;
            }
            let overlap = f64::min(left.max, right.max) - f64::max(left.min, right.min);
            if overlap >= 72.0 && (left.line - right.line).abs() <= 10.0 {
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// ROUTE_SEPARATION_DISTANCES (L670)
// ---------------------------------------------------------------------------

/// Port of JS `ROUTE_SEPARATION_DISTANCES`.
///
/// The loop in `separateCloseParallelRoutes` iterates these in order and keeps
/// the FIRST distance that yields the best score.  The positive distances are
/// tried before negative ones (push-away before pull-toward).
pub const ROUTE_SEPARATION_DISTANCES: [f64; 24] = [
    5.0, 7.0, 9.0, 11.0, 13.0, 15.0, 18.0, 24.0, 30.0, 36.0, 48.0, 60.0,
    -5.0, -7.0, -9.0, -11.0, -13.0, -15.0, -18.0, -24.0, -30.0, -36.0, -48.0, -60.0,
];

// ---------------------------------------------------------------------------
// crossingPairKey (L672)
// ---------------------------------------------------------------------------

/// Port of JS `crossingPairKey(leftRouteIndex, rightRouteIndex)`.
///
/// Returns a canonical `"min:max"` string key for a pair of route indices.
pub fn crossing_pair_key(left_route_index: usize, right_route_index: usize) -> String {
    if left_route_index < right_route_index {
        format!("{}:{}", left_route_index, right_route_index)
    } else {
        format!("{}:{}", right_route_index, left_route_index)
    }
}

// ---------------------------------------------------------------------------
// RouteSetStats — return type of routeSetStats
// ---------------------------------------------------------------------------

/// Returned by `route_set_stats`.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteSetStats {
    pub repeated_crossings: usize,
    pub shared_segments: usize,
}

// ---------------------------------------------------------------------------
// routeSetStats (L678)
// ---------------------------------------------------------------------------

/// Port of JS `routeSetStats(routeById)`.
///
/// Counts shared segments (exact co-linear overlaps > 1px) and
/// repeated-crossing pairs (pairs of routes that cross more than once — each
/// extra crossing beyond the first counts as one "repeated crossing").
///
/// Uses `HOP_RADIUS` from `route_rendering` to match the JS
/// `HOP_RADIUS` import.
pub fn route_set_stats(route_by_id: &[(String, RouteData)]) -> RouteSetStats {
    use crate::route_rendering::HOP_RADIUS;
    use indexmap::IndexMap as IxMap;

    let mut crossings: IxMap<String, usize> = IxMap::new();
    let mut shared_segments = 0usize;

    for left_route_index in 0..route_by_id.len() {
        for right_route_index in left_route_index + 1..route_by_id.len() {
            for left in axis_aligned_route_segments(&route_by_id[left_route_index].1) {
                for right in axis_aligned_route_segments(&route_by_id[right_route_index].1) {
                    if left.orientation == right.orientation {
                        if left.line != right.line {
                            continue;
                        }
                        let overlap =
                            f64::min(left.max, right.max) - f64::max(left.min, right.min);
                        if overlap > 1.0 {
                            shared_segments += 1;
                        }
                        continue;
                    }
                    // Crossing detection: one horizontal, one vertical.
                    let (horizontal, vertical) = if left.orientation == "horizontal" {
                        (&left, &right)
                    } else {
                        (&right, &left)
                    };
                    if vertical.line > horizontal.min + HOP_RADIUS
                        && vertical.line < horizontal.max - HOP_RADIUS
                        && horizontal.line > vertical.min + HOP_RADIUS
                        && horizontal.line < vertical.max - HOP_RADIUS
                    {
                        let key = crossing_pair_key(left_route_index, right_route_index);
                        *crossings.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let repeated_crossings: usize = crossings
        .values()
        .map(|&count| count.saturating_sub(1))
        .sum();

    RouteSetStats { repeated_crossings, shared_segments }
}

// ---------------------------------------------------------------------------
// routeSeparationScore (L713)
// ---------------------------------------------------------------------------

/// Port of JS `routeSeparationScore({ nextCloseCount, nextStats, nextPairCloseCount, candidate, distance })`.
///
/// Returns a 6-element lexicographic score array.  Lower is better on every element.
/// The `distance` term uses `f64::abs` matching JS `Math.abs`.
pub fn route_separation_score(
    next_close_count: usize,
    next_stats: &RouteSetStats,
    next_pair_close_count: usize,
    candidate_bends: usize,
    distance: f64,
) -> [f64; 6] {
    [
        next_close_count as f64,
        next_stats.shared_segments as f64,
        next_stats.repeated_crossings as f64,
        next_pair_close_count as f64,
        candidate_bends as f64,
        distance.abs(),
    ]
}

// ---------------------------------------------------------------------------
// isBetterRouteSeparation (L724)
// ---------------------------------------------------------------------------

/// Port of JS `isBetterRouteSeparation(left, right)`.
///
/// Returns `true` when `left.score` is lexicographically strictly less than
/// `right.score` (`right = None` → left always wins).
pub fn is_better_route_separation(
    left_score: &[f64; 6],
    right_score: Option<&[f64; 6]>,
) -> bool {
    let right = match right_score {
        None => return true,
        Some(r) => r,
    };
    for index in 0..left_score.len() {
        if left_score[index] != right[index] {
            return left_score[index] < right[index];
        }
    }
    false
}

// ---------------------------------------------------------------------------
// totalBendsForRoutes (L732)
// ---------------------------------------------------------------------------

/// Port of JS `totalBendsForRoutes(routeById)`.
pub fn total_bends_for_routes(route_by_id: &[(String, RouteData)]) -> usize {
    route_by_id.iter().map(|(_, r)| r.bends).sum()
}

// ---------------------------------------------------------------------------
// routeSetScore (L736)
// ---------------------------------------------------------------------------

/// Port of JS `routeSetScore(routeById)`.
///
/// Returns a 4-element lexicographic score.  Lower is better on every element.
pub fn route_set_score(route_by_id: &[(String, RouteData)]) -> [f64; 4] {
    let stats = route_set_stats(route_by_id);
    [
        close_parallel_run_count_for_routes(route_by_id) as f64,
        stats.shared_segments as f64,
        stats.repeated_crossings as f64,
        total_bends_for_routes(route_by_id) as f64,
    ]
}

// ---------------------------------------------------------------------------
// isBetterRouteSet (L746)
// ---------------------------------------------------------------------------

/// Port of JS `isBetterRouteSet(left, right)`.
pub fn is_better_route_set(left_score: &[f64; 4], right_score: Option<&[f64; 4]>) -> bool {
    let right = match right_score {
        None => return true,
        Some(r) => r,
    };
    for index in 0..left_score.len() {
        if left_score[index] != right[index] {
            return left_score[index] < right[index];
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Relationship descriptor for separation functions
// ---------------------------------------------------------------------------

/// Minimal relationship fields required by the Pass C2 separation functions.
pub struct SeparationRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
}

// ---------------------------------------------------------------------------
// RerouteCallback — trait for the reroutedAgainstRouteSet call-out
// ---------------------------------------------------------------------------

/// Allows `separate_close_parallel_routes` to call back into the full planner
/// for the "reroute against route-set" step (first-distance-only, from the
/// pass C orchestration layer that will be wired in a later pass).
///
/// The callback receives the route ID being rerouted and the current route
/// set (all other routes), and must return `Some(RouteData)` if a better
/// route was found or `None` to skip.
pub trait RerouteCallback {
    fn reroute_against_route_set(
        &self,
        route_id: &str,
        route_by_id: &[(String, RouteData)],
    ) -> Option<RouteData>;
}

/// A no-op implementation used when the full planner is not yet wired.
pub struct NoopReroute;
impl RerouteCallback for NoopReroute {
    fn reroute_against_route_set(&self, _route_id: &str, _route_by_id: &[(String, RouteData)]) -> Option<RouteData> {
        None
    }
}

// ---------------------------------------------------------------------------
// separateCloseParallelRoutes (L758)
// ---------------------------------------------------------------------------

/// Port of JS `separateCloseParallelRoutes(plannedRawRoutes, input)`.
///
/// Iteratively finds the first close-parallel segment pair and tries a set of
/// candidate shifts to separate them, keeping the attempt that most improves
/// the route set score.  Stops early when no more close-parallel pairs exist
/// or when no candidate improves the set.
///
/// # Arguments
/// * `planned_raw_routes` — ordered `(id, route)` list, same as JS Map iteration order.
/// * `relationships` — relationship list for `route_pair_index` and `reroute`.
/// * `node_rects` — node geometry for endpoint-shift helpers.
/// * `fixed_ports` — which nodes have fixed port positions.
/// * `reroute` — callback for the full-planner reroute step (first distance only).
///
/// # Faithful notes
/// - Distances are tried in `ROUTE_SEPARATION_DISTANCES` order; the loop keeps
///   the *first* distance that yields `is_better_route_separation == true` for
///   a given `best` winner (not globally first — it accumulates a per-attempt
///   `best` across all distances and options, then applies the winner).
/// - The `reroute` step is attempted only when `distance.abs() == ROUTE_SEPARATION_DISTANCES[0]` (5.0).
/// - After each successful step, if `is_better_route_set` the global `best_route_set` is updated.
pub fn separate_close_parallel_routes<R: RerouteCallback>(
    planned_raw_routes: &[(String, RouteData)],
    relationships: &[SeparationRelationship],
    node_rects: &IndexMap<String, Rect>,
    fixed_ports: &IndexMap<String, bool>,
    reroute: &R,
) -> Vec<(String, RouteData)> {
    let rel_by_id: IndexMap<&str, &SeparationRelationship> =
        relationships.iter().map(|r| (r.id.as_str(), r)).collect();

    // Clone the working set (mirrors JS `new Map(plannedRawRoutes)`).
    let mut route_by_id: Vec<(String, RouteData)> = planned_raw_routes.to_vec();

    let initial_score = route_set_score(&route_by_id);
    let mut best_route_set = (route_by_id.clone(), initial_score);

    let max_attempts = usize::max(8, planned_raw_routes.len() * 12);

    for _attempt in 0..max_attempts {
        let pair = match close_parallel_segment_pair(&route_by_id) {
            Some(p) => p,
            None => break,
        };

        let current_close_count = close_parallel_run_count_for_routes(&route_by_id);
        let current_stats = route_set_stats(&route_by_id);

        let current_pair_close_count = {
            let left_r = route_by_id.iter().find(|(id, _)| id == &pair.left_id).map(|(_, r)| r.clone());
            let right_r = route_by_id.iter().find(|(id, _)| id == &pair.right_id).map(|(_, r)| r.clone());
            match (left_r, right_r) {
                (Some(l), Some(r)) => close_parallel_run_count_between(&l, &r),
                _ => 0,
            }
        };

        struct OptionEntry {
            route_id: String,
            segment: RouteSegment,
            other_line: f64,
        }
        let options: [OptionEntry; 2] = [
            OptionEntry { route_id: pair.left_id.clone(), segment: pair.left.clone(), other_line: pair.right.line },
            OptionEntry { route_id: pair.right_id.clone(), segment: pair.right.clone(), other_line: pair.left.line },
        ];

        // best over this attempt
        let mut best_score: Option<[f64; 6]> = None;
        let mut best_entry: Option<(String, RouteData)> = None; // (route_id, candidate)

        for option in &options {
            let route = match route_by_id.iter().find(|(id, _)| *id == option.route_id) {
                Some((_, r)) => r.clone(),
                None => continue,
            };
            let rel = match rel_by_id.get(option.route_id.as_str()) {
                Some(r) => *r,
                None => continue,
            };
            let direction: f64 = if option.segment.line >= option.other_line { 1.0 } else { -1.0 };

            for &distance in &ROUTE_SEPARATION_DISTANCES {
                let delta = distance * direction;

                // Build candidates list (mirrors JS `.filter(Boolean)` after building array).
                let mut candidates: Vec<RouteData> = [
                    shifted_direct_endpoint_route(
                        &route, &rel.from, &rel.to, node_rects, fixed_ports, &option.segment, delta,
                    ),
                    shifted_internal_segment_route(&route, &option.segment, delta),
                    shifted_endpoint_segment_route(
                        &route, &rel.from, &rel.to, node_rects, fixed_ports, &option.segment, delta,
                    ),
                ]
                .into_iter()
                .flatten()
                .map(|c| route_with_endpoint_stubs(&c, &rel.from, &rel.to, node_rects))
                .collect();

                // Reroute step: only for the first distance magnitude.
                if distance.abs() == ROUTE_SEPARATION_DISTANCES[0] {
                    if let Some(rerouted) = reroute.reroute_against_route_set(&option.route_id, &route_by_id) {
                        candidates.push(route_with_endpoint_stubs(&rerouted, &rel.from, &rel.to, node_rects));
                    }
                }

                for candidate in &candidates {
                    // Filter: must exit perpendicularly.
                    if !route_endpoints_are_perpendicular(candidate, &rel.from, &rel.to, node_rects) {
                        continue;
                    }
                    // Filter: must not collide with non-endpoint nodes.
                    {
                        let simple_rel = Relationship { from: rel.from.as_str(), to: rel.to.as_str() };
                        let simple_input = RouteInput {
                            visible_node_ids: &route_by_id.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
                            node_rects,
                        };
                        if route_collides_with_non_endpoints(candidate, &simple_rel, &simple_input) {
                            continue;
                        }
                        if route_has_endpoint_traversal(candidate, &simple_rel, &simple_input) {
                            continue;
                        }
                    }

                    // Temporarily apply candidate to measure stats.
                    let temp_route_by_id: Vec<(String, RouteData)> =
                        route_by_id.iter().map(|(id, r)| {
                            if *id == option.route_id {
                                (id.clone(), candidate.clone())
                            } else {
                                (id.clone(), r.clone())
                            }
                        }).collect();

                    let next_close_count = close_parallel_run_count_for_routes(&temp_route_by_id);
                    let next_stats = route_set_stats(&temp_route_by_id);
                    let next_pair_close_count = {
                        let left_r = temp_route_by_id.iter().find(|(id, _)| id == &pair.left_id).map(|(_, r)| r.clone());
                        let right_r = temp_route_by_id.iter().find(|(id, _)| id == &pair.right_id).map(|(_, r)| r.clone());
                        match (left_r, right_r) {
                            (Some(l), Some(r)) => close_parallel_run_count_between(&l, &r),
                            _ => 0,
                        }
                    };

                    // Guard: must not increase shared segments.
                    if next_stats.shared_segments > current_stats.shared_segments {
                        continue;
                    }
                    let improves_shared = next_stats.shared_segments < current_stats.shared_segments;
                    let improves_close_runs = next_close_count < current_close_count;
                    let improves_pair = next_pair_close_count < current_pair_close_count;
                    if !improves_shared && !improves_close_runs && !improves_pair {
                        continue;
                    }
                    // Guard: must not add too many new repeated crossings.
                    if next_stats.repeated_crossings > current_stats.repeated_crossings + 4 {
                        continue;
                    }

                    let score = route_separation_score(
                        next_close_count,
                        &next_stats,
                        next_pair_close_count,
                        candidate.bends,
                        distance,
                    );
                    if is_better_route_separation(&score, best_score.as_ref()) {
                        best_score = Some(score);
                        best_entry = Some((option.route_id.clone(), candidate.clone()));
                    }
                }
            }
        }

        match best_entry {
            None => break,
            Some((best_route_id, best_candidate)) => {
                // Apply the best candidate.
                for (id, r) in &mut route_by_id {
                    if *id == best_route_id {
                        *r = best_candidate.clone();
                        break;
                    }
                }
                // Update global best if improved.
                let next_score = route_set_score(&route_by_id);
                if is_better_route_set(&next_score, Some(&best_route_set.1)) {
                    best_route_set = (route_by_id.clone(), next_score);
                }
            }
        }
    }

    // Return in the same order as `planned_raw_routes`, picking from best_route_set.
    planned_raw_routes
        .iter()
        .map(|(rel_id, _)| {
            let route = best_route_set
                .0
                .iter()
                .find(|(id, _)| id == rel_id)
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| {
                    planned_raw_routes
                        .iter()
                        .find(|(id, _)| id == rel_id)
                        .map(|(_, r)| r.clone())
                        .expect("relationship id always present in planned_raw_routes")
                });
            (rel_id.clone(), route)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// alternateMiddleDoglegRoutes (L833)
// ---------------------------------------------------------------------------

/// Port of JS `alternateMiddleDoglegRoutes(route)`.
///
/// For a 5-point route that has a horizontal or vertical endpoint dogleg,
/// returns up to 2 alternative 5-point and 6-point routes that resolve the
/// dogleg by moving the intermediate bend.
///
/// Returns an empty `Vec` when the route is not 5 points or does not match
/// either dogleg pattern.
pub fn alternate_middle_dogleg_routes(route: &RouteData) -> Vec<RouteData> {
    if route.points.len() != 5 {
        return Vec::new();
    }
    let start = &route.points[0];
    let source_stub = &route.points[1];
    let middle_a = &route.points[2];
    let target_stub = &route.points[3];
    let end = &route.points[4];

    // Horizontal endpoint dogleg:
    // start.y == sourceStub.y == middleA.y  AND  middleA.x == targetStub.x  AND  targetStub.y == end.y
    let horizontal_dogleg = start.y == source_stub.y
        && source_stub.y == middle_a.y
        && middle_a.x == target_stub.x
        && target_stub.y == end.y;
    if horizontal_dogleg && source_stub.x != target_stub.x {
        let gutter_y = (source_stub.y + target_stub.y) / 2.0;
        let alt1 = route_with_points(
            route,
            vec![
                start.clone(),
                source_stub.clone(),
                Point { x: source_stub.x, y: target_stub.y },
                target_stub.clone(),
                end.clone(),
            ],
            route.controls.clone(),
        );
        let alt2 = route_with_points(
            route,
            vec![
                start.clone(),
                source_stub.clone(),
                Point { x: source_stub.x, y: gutter_y },
                Point { x: target_stub.x, y: gutter_y },
                target_stub.clone(),
                end.clone(),
            ],
            route.controls.clone(),
        );
        return vec![alt1, alt2];
    }

    // Vertical endpoint dogleg:
    // start.x == sourceStub.x == middleA.x  AND  middleA.y == targetStub.y  AND  targetStub.x == end.x
    let vertical_dogleg = start.x == source_stub.x
        && source_stub.x == middle_a.x
        && middle_a.y == target_stub.y
        && target_stub.x == end.x;
    if vertical_dogleg && source_stub.y != target_stub.y {
        let gutter_x = (source_stub.x + target_stub.x) / 2.0;
        let alt1 = route_with_points(
            route,
            vec![
                start.clone(),
                source_stub.clone(),
                Point { x: target_stub.x, y: source_stub.y },
                target_stub.clone(),
                end.clone(),
            ],
            route.controls.clone(),
        );
        let alt2 = route_with_points(
            route,
            vec![
                start.clone(),
                source_stub.clone(),
                Point { x: gutter_x, y: source_stub.y },
                Point { x: gutter_x, y: target_stub.y },
                target_stub.clone(),
                end.clone(),
            ],
            route.controls.clone(),
        );
        return vec![alt1, alt2];
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// spreadUnitSlots (Pass C3, L903)
// ---------------------------------------------------------------------------

/// Port of JS `spreadUnitSlots(halfWidths, sideLength)`.
///
/// Even-gap slot offsets (relative to face centre) for units of the given
/// half-widths.  When all half-widths are zero this reduces exactly to
/// `endpointSpreadOffset` (lone-mount faces stay byte-identical).  When units
/// cannot fit (slack ≤ 0) falls back to even centres so the per-face guard
/// can handle the squeeze.
pub fn spread_unit_slots(half_widths: &[f64], side_length: f64) -> Vec<f64> {
    let count = half_widths.len();
    let content: f64 = half_widths.iter().map(|&hw| 2.0 * hw).sum();
    let slack = side_length - content;
    if slack <= 0.0 {
        // Fall back: even centres (same as endpointSpreadOffset)
        return half_widths
            .iter()
            .enumerate()
            .map(|(index, _)| ((index + 1) as f64 / (count + 1) as f64 - 0.5) * side_length)
            .collect();
    }
    let gap = slack / (count + 1) as f64;
    let mut slots = Vec::with_capacity(count);
    let mut cursor = -side_length / 2.0;
    for &hw in half_widths {
        cursor += gap + hw;
        slots.push(cursor);
        cursor += hw;
    }
    slots
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

    // -----------------------------------------------------------------------
    // Pass C1 tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // sideEndpointKey
    // -----------------------------------------------------------------------

    #[test]
    fn side_endpoint_key_nul_separator() {
        // Node: sideEndpointKey('node-A','left') → charCodes include NUL at pos 6
        // confirmed: [110,111,100,101,45,65,0,108,101,102,116]
        let k = side_endpoint_key("node-A", "left");
        let bytes: Vec<u8> = k.bytes().collect();
        // NUL byte at position 6 (after "node-A")
        assert_eq!(bytes[6], 0, "separator must be NUL");
        assert_eq!(k, "node-A\0left");
    }

    #[test]
    fn side_endpoint_key_same_same() {
        // Node: sideEndpointKey('A','right') === sideEndpointKey('A','right')
        assert_eq!(side_endpoint_key("A", "right"), side_endpoint_key("A", "right"));
    }

    #[test]
    fn side_endpoint_key_same_diff() {
        // Node: sideEndpointKey('A','right') !== sideEndpointKey('A','left')
        assert_ne!(side_endpoint_key("A", "right"), side_endpoint_key("A", "left"));
    }

    // -----------------------------------------------------------------------
    // renderedAxisAlignedSegments
    // -----------------------------------------------------------------------

    #[test]
    fn rendered_axis_aligned_segments_l_shape() {
        // Node: [{x:0,y:0},{x:100,y:0},{x:100,y:50}]
        // → [{"orientation":"horizontal","line":0,"min":0,"max":100},
        //    {"orientation":"vertical","line":100,"min":0,"max":50}]
        let pts = vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)];
        let segs = rendered_axis_aligned_segments(&pts);
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
    fn rendered_axis_aligned_segments_vertical() {
        // Node: [{x:10,y:5},{x:10,y:80}]
        // → [{"orientation":"vertical","line":10,"min":5,"max":80}]
        let pts = vec![pt(10.0, 5.0), pt(10.0, 80.0)];
        let segs = rendered_axis_aligned_segments(&pts);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].orientation, "vertical");
        assert_eq!(segs[0].line, 10.0);
        assert_eq!(segs[0].min, 5.0);
        assert_eq!(segs[0].max, 80.0);
    }

    #[test]
    fn rendered_axis_aligned_segments_empty() {
        // Node: null → [], [] → []
        assert!(rendered_axis_aligned_segments(&[]).is_empty());
        assert!(rendered_axis_aligned_segments(&[pt(0.0, 0.0)]).is_empty());
    }

    // -----------------------------------------------------------------------
    // finalSharedSegmentStats
    // -----------------------------------------------------------------------

    #[test]
    fn final_shared_segment_stats_horizontal_overlap() {
        // Node: finalSharedSegmentStats(rA, [rA,rB,rC]) = {count:1, length:50}
        // rA=[{0,0}→{100,0}], rB=[{50,0}→{200,0}] overlap=50, rC=[{0,5}→{100,5}] diff line
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_b = orthogonal_route(vec![pt(50.0, 0.0), pt(200.0, 0.0)]);
        let route_c = orthogonal_route(vec![pt(0.0, 5.0), pt(100.0, 5.0)]);
        let all = vec![route_a.clone(), route_b, route_c];
        let stats = final_shared_segment_stats(&route_a, &all, 0);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.length, 50.0);
    }

    #[test]
    fn final_shared_segment_stats_no_overlap() {
        // Node: finalSharedSegmentStats(rA, [rA,rC]) = {count:0, length:0}
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_c = orthogonal_route(vec![pt(0.0, 5.0), pt(100.0, 5.0)]);
        let all = vec![route_a.clone(), route_c];
        let stats = final_shared_segment_stats(&route_a, &all, 0);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.length, 0.0);
    }

    #[test]
    fn final_shared_segment_stats_vertical_overlap() {
        // Node: finalSharedSegmentStats(rD, [rD,rE]) = {count:1, length:50}
        // rD=[{10,0}→{10,100}], rE=[{10,50}→{10,150}] overlap=50
        let route_d = orthogonal_route(vec![pt(10.0, 0.0), pt(10.0, 100.0)]);
        let route_e = orthogonal_route(vec![pt(10.0, 50.0), pt(10.0, 150.0)]);
        let all = vec![route_d.clone(), route_e];
        let stats = final_shared_segment_stats(&route_d, &all, 0);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.length, 50.0);
    }

    // -----------------------------------------------------------------------
    // sharedSegmentCount
    // -----------------------------------------------------------------------

    #[test]
    fn shared_segment_count_one_overlap() {
        // Node: sharedSegmentCount(rA, [rB]) = 1
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_b = orthogonal_route(vec![pt(50.0, 0.0), pt(200.0, 0.0)]);
        assert_eq!(shared_segment_count(&route_a, &[route_b]), 1);
    }

    #[test]
    fn shared_segment_count_no_overlap() {
        // Node: sharedSegmentCount(rA, [rC]) = 0  (different y)
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_c = orthogonal_route(vec![pt(0.0, 5.0), pt(100.0, 5.0)]);
        assert_eq!(shared_segment_count(&route_a, &[route_c]), 0);
    }

    #[test]
    fn shared_segment_count_vertical() {
        // Node: sharedSegmentCount(rD, [rE]) = 1
        let route_d = orthogonal_route(vec![pt(10.0, 0.0), pt(10.0, 100.0)]);
        let route_e = orthogonal_route(vec![pt(10.0, 50.0), pt(10.0, 150.0)]);
        assert_eq!(shared_segment_count(&route_d, &[route_e]), 1);
    }

    // -----------------------------------------------------------------------
    // nonEndpointNodeCollisionCount
    // -----------------------------------------------------------------------

    #[test]
    fn non_endpoint_node_collision_count_skip_endpoints() {
        // Route passes through a node that is neither from nor to → count=1.
        // Route from A(0,0,50,50) to B(200,0,50,50) passes through C(100,0,20,20).
        let route = orthogonal_route(vec![pt(50.0, 10.0), pt(110.0, 10.0), pt(250.0, 10.0)]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 50.0, 50.0));
        // C sits at x=90..110, y=0..20; the route crosses through it at y=10
        node_rects.insert("C".into(), rect(90.0, 0.0, 20.0, 20.0));
        let visible = vec!["A".into(), "B".into(), "C".into()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        let count = non_endpoint_node_collision_count(&route, &rel, &input);
        assert_eq!(count, 1);
    }

    #[test]
    fn non_endpoint_node_collision_count_endpoints_skipped() {
        // Route only goes through its own endpoint nodes → count=0.
        let route = orthogonal_route(vec![pt(50.0, 10.0), pt(200.0, 10.0)]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".into(), "B".into()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        assert_eq!(non_endpoint_node_collision_count(&route, &rel, &input), 0);
    }

    // -----------------------------------------------------------------------
    // endpointStubRoute
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_stub_route_extends_short_start_stub() {
        // Node: shortRoute=[{0,25},{5,25},{5,50},{100,50}], from rect {0,0,50,50}
        // anchor (0,25) is on left side (x=rect.x=0). stub=5 < PORT_STUB=18.
        // After: adjacent→(-18,25), elbow x matches oldAdj(5)→-18.
        // Node confirmed: [{x:0,y:25},{x:-18,y:25},{x:-18,y:50},{x:100,y:50}]
        let route = orthogonal_route(vec![
            pt(0.0, 25.0), pt(5.0, 25.0), pt(5.0, 50.0), pt(100.0, 50.0),
        ]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(80.0, 30.0, 50.0, 50.0));
        let result = endpoint_stub_route(&route, "A", "B", &node_rects, 0);
        // After route_with_points simplification the points list may be reordered;
        // check the key values directly.
        assert_eq!(result.points[0], pt(0.0, 25.0), "anchor unchanged");
        assert_eq!(result.points[1], pt(-18.0, 25.0), "adjacent extended to PORT_STUB");
        assert_eq!(result.points[2].x, -18.0, "elbow x follows adjacent");
    }

    #[test]
    fn endpoint_stub_route_long_stub_unchanged() {
        // Node: endpointStubRoute with stub already >= PORT_STUB → route unchanged
        let route = orthogonal_route(vec![
            pt(0.0, 25.0), pt(-20.0, 25.0), pt(-20.0, 50.0), pt(100.0, 50.0),
        ]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(80.0, 30.0, 50.0, 50.0));
        let result = endpoint_stub_route(&route, "A", "B", &node_rects, 0);
        // stub length=20 >= 18 → unchanged
        assert_eq!(result.points[1], pt(-20.0, 25.0));
    }

    #[test]
    fn endpoint_stub_route_too_few_points_unchanged() {
        // Node: route with < 3 points → returned unchanged
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let node_rects: IndexMap<String, Rect> = IndexMap::new();
        let result = endpoint_stub_route(&route, "A", "B", &node_rects, 0);
        assert_eq!(result.points, route.points);
    }

    // -----------------------------------------------------------------------
    // routeWithEndpointStubs
    // -----------------------------------------------------------------------

    #[test]
    fn route_with_endpoint_stubs_applies_both_ends() {
        // Route with short stubs at both ends on the same y; after extension both
        // stubs move outward but simplify_orthogonal_points collapses the now-
        // collinear run [{0,25},{-18,25},{195,25},{200,25}] → [{0,25},{200,25}].
        //
        // Node: routeWithEndpointStubs([{0,25},{5,25},{195,25},{200,25}], A, B)
        //   → after start-stub → routeWithPoints collapses → [{0,25},{200,25}]
        //   → then end-stub: 2 pts < 3 → unchanged → [{0,25},{200,25}]
        // Confirmed by node /tmp/test_c1_both_ends.js:
        //   Full result: [{"x":0,"y":25},{"x":200,"y":25}]
        let route = orthogonal_route(vec![
            pt(0.0, 25.0), pt(5.0, 25.0), pt(195.0, 25.0), pt(200.0, 25.0),
        ]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(150.0, 0.0, 50.0, 50.0));
        let result = route_with_endpoint_stubs(&route, "A", "B", &node_rects);
        // Collinear simplification collapses to 2 points.
        assert_eq!(result.points.len(), 2);
        assert_eq!(result.points[0], pt(0.0, 25.0));
        assert_eq!(result.points[1], pt(200.0, 25.0));
    }

    // -----------------------------------------------------------------------
    // Pass C2 tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // axisAlignedRouteSegments
    // -----------------------------------------------------------------------

    #[test]
    fn axis_aligned_route_segments_l_shape() {
        // Node: route [{x:0,y:0},{x:100,y:0},{x:100,y:50}]
        // → [{ orientation:"horizontal", line:0, min:0, max:100, index:0 },
        //    { orientation:"vertical",   line:100, min:0, max:50, index:1 }]
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)]);
        let segs = axis_aligned_route_segments(&route);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 0.0);
        assert_eq!(segs[0].max, 100.0);
        assert_eq!(segs[0].index, 0);
        assert_eq!(segs[1].orientation, "vertical");
        assert_eq!(segs[1].line, 100.0);
        assert_eq!(segs[1].min, 0.0);
        assert_eq!(segs[1].max, 50.0);
        assert_eq!(segs[1].index, 1);
    }

    #[test]
    fn axis_aligned_route_segments_empty() {
        let route = orthogonal_route(vec![]);
        assert!(axis_aligned_route_segments(&route).is_empty());
    }

    // -----------------------------------------------------------------------
    // closeParallelSegmentPair
    // -----------------------------------------------------------------------

    #[test]
    fn close_parallel_segment_pair_found_close_parallel() {
        // Node: two horizontal routes at y=100 and y=105, spanning x=0..200
        // → close parallel (distance=5, overlap=200 >= 72)
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 105.0), pt(200.0, 105.0)]);
        let route_by_id = vec![("r1".into(), r1), ("r2".into(), r2)];
        let result = close_parallel_segment_pair(&route_by_id);
        assert!(result.is_some());
        let pair = result.unwrap();
        assert_eq!(pair.left_id, "r1");
        assert_eq!(pair.right_id, "r2");
        assert_eq!(pair.left.orientation, "horizontal");
        assert_eq!(pair.left.line, 100.0);
        assert_eq!(pair.right.line, 105.0);
    }

    #[test]
    fn close_parallel_segment_pair_exact_shared() {
        // Node: two routes on same horizontal line, overlapping > 1px
        // distance=0, overlap=100 > 1 → exactSharedSegment
        let r3 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r4 = orthogonal_route(vec![pt(50.0, 100.0), pt(150.0, 100.0)]);
        let route_by_id = vec![("r3".into(), r3), ("r4".into(), r4)];
        let result = close_parallel_segment_pair(&route_by_id);
        assert!(result.is_some());
    }

    #[test]
    fn close_parallel_segment_pair_not_found_too_far() {
        // Node: distance=11 > 10 → no close parallel
        let r5 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r6 = orthogonal_route(vec![pt(0.0, 111.0), pt(200.0, 111.0)]);
        let route_by_id = vec![("r5".into(), r5), ("r6".into(), r6)];
        assert!(close_parallel_segment_pair(&route_by_id).is_none());
    }

    #[test]
    fn close_parallel_segment_pair_not_found_short_overlap() {
        // Node: overlap=50 < 72 → no close parallel
        let r7 = orthogonal_route(vec![pt(0.0, 100.0), pt(50.0, 100.0)]);
        let r8 = orthogonal_route(vec![pt(0.0, 105.0), pt(50.0, 105.0)]);
        let route_by_id = vec![("r7".into(), r7), ("r8".into(), r8)];
        assert!(close_parallel_segment_pair(&route_by_id).is_none());
    }

    // -----------------------------------------------------------------------
    // shiftedInternalSegmentRoute
    // -----------------------------------------------------------------------

    #[test]
    fn shifted_internal_segment_route_middle_vertical() {
        // Node: Z-route [{x:0,y:50},{x:50,y:50},{x:50,y:100},{x:100,y:100}]
        // segment index=1 (vertical, x=50), delta=10 → shifts index 1,2 x by 10
        // → [{0,50},{60,50},{60,100},{100,100}]
        let route = orthogonal_route(vec![
            pt(0.0, 50.0), pt(50.0, 50.0), pt(50.0, 100.0), pt(100.0, 100.0),
        ]);
        let seg = RouteSegment { index: 1, orientation: "vertical", line: 50.0, min: 50.0, max: 100.0 };
        let result = shifted_internal_segment_route(&route, &seg, 10.0);
        assert!(result.is_some());
        let r = result.unwrap();
        // After simplification: points may be reordered but the middle x should shift
        // Node (js): [{x:0,y:50},{x:60,y:50},{x:60,y:100},{x:100,y:100}]
        assert_eq!(r.points.len(), 4);
        assert_eq!(r.points[1].x, 60.0);
        assert_eq!(r.points[2].x, 60.0);
    }

    #[test]
    fn shifted_internal_segment_route_endpoint_returns_none() {
        // Segment at index 0 (endpoint) → None
        let route = orthogonal_route(vec![pt(0.0, 50.0), pt(100.0, 50.0), pt(100.0, 100.0)]);
        let seg = RouteSegment { index: 0, orientation: "horizontal", line: 50.0, min: 0.0, max: 100.0 };
        assert!(shifted_internal_segment_route(&route, &seg, 10.0).is_none());
    }

    // -----------------------------------------------------------------------
    // routePairIndex
    // -----------------------------------------------------------------------

    #[test]
    fn route_pair_index_basic() {
        // Node (confirmed above):
        // rels=[{r1,A→B},{r2,B→A},{r3,A→B},{r4,C→D}]
        // routePairIndex(r1) = 0, (r2) = 1, (r3) = 2, (r4) = 0
        let rels: Vec<SeparationRelationship> = vec![
            SeparationRelationship { id: "r1".into(), from: "A".into(), to: "B".into() },
            SeparationRelationship { id: "r2".into(), from: "B".into(), to: "A".into() },
            SeparationRelationship { id: "r3".into(), from: "A".into(), to: "B".into() },
            SeparationRelationship { id: "r4".into(), from: "C".into(), to: "D".into() },
        ];
        assert_eq!(route_pair_index("r1", "A", "B", &rels), 0);
        assert_eq!(route_pair_index("r2", "B", "A", &rels), 1);
        assert_eq!(route_pair_index("r3", "A", "B", &rels), 2);
        assert_eq!(route_pair_index("r4", "C", "D", &rels), 0);
    }

    // -----------------------------------------------------------------------
    // routeEndpointsArePerpendicular
    // -----------------------------------------------------------------------

    #[test]
    fn route_endpoints_are_perpendicular_true() {
        // Node: A at (0,0,100,50), B at (200,0,100,50)
        // Route exits A's right side (x=100) horizontally toward B's left side (x=200).
        // Node: routeEndpointsArePerpendicular(route, rel, input) = true
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 100.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 100.0, 50.0));
        let route = orthogonal_route(vec![pt(100.0, 25.0), pt(150.0, 25.0), pt(200.0, 25.0)]);
        assert!(route_endpoints_are_perpendicular(&route, "A", "B", &node_rects));
    }

    #[test]
    fn route_endpoints_are_perpendicular_false() {
        // Node: exit not parallel to side → false
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 100.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 100.0, 50.0));
        // Exits right of A (y=25) but adjacent is at y=50 → not perpendicular
        let route = orthogonal_route(vec![pt(100.0, 25.0), pt(150.0, 50.0), pt(200.0, 25.0)]);
        // After simplification this might be 3 points but the perpendicular check still fails.
        // Point 0 is on right side of A (x=100), adjacent point[1].y must equal point[0].y.
        // point[1].y=50 != point[0].y=25 → false.
        assert!(!route_endpoints_are_perpendicular(&route, "A", "B", &node_rects));
    }

    // -----------------------------------------------------------------------
    // closeParallelRunCountForRoutes
    // -----------------------------------------------------------------------

    #[test]
    fn close_parallel_run_count_for_routes_two_routes() {
        // Node: 2 close parallel horizontal routes (distance=5, overlap=200)
        // → count = 1
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 105.0), pt(200.0, 105.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        assert_eq!(close_parallel_run_count_for_routes(&rbd), 1);
    }

    #[test]
    fn close_parallel_run_count_for_routes_none() {
        // Node: distance=11 → 0
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 111.0), pt(200.0, 111.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        assert_eq!(close_parallel_run_count_for_routes(&rbd), 0);
    }

    // -----------------------------------------------------------------------
    // closeParallelRunCountBetween
    // -----------------------------------------------------------------------

    #[test]
    fn close_parallel_run_count_between_pair() {
        // Node: same as above but testing the between version
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 105.0), pt(200.0, 105.0)]);
        assert_eq!(close_parallel_run_count_between(&r1, &r2), 1);
    }

    #[test]
    fn close_parallel_run_count_between_none() {
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 111.0), pt(200.0, 111.0)]);
        assert_eq!(close_parallel_run_count_between(&r1, &r2), 0);
    }

    // -----------------------------------------------------------------------
    // crossingPairKey
    // -----------------------------------------------------------------------

    #[test]
    fn crossing_pair_key_canonical() {
        // Node: crossingPairKey(0,1)="0:1", crossingPairKey(1,0)="0:1"
        assert_eq!(crossing_pair_key(0, 1), "0:1");
        assert_eq!(crossing_pair_key(1, 0), "0:1");
        assert_eq!(crossing_pair_key(2, 3), "2:3");
        assert_eq!(crossing_pair_key(3, 2), "2:3");
    }

    // -----------------------------------------------------------------------
    // routeSetStats
    // -----------------------------------------------------------------------

    #[test]
    fn route_set_stats_shared_segment() {
        // Node: two routes sharing horizontal segment at y=100, overlap=100 > 1
        // → {repeatedCrossings:0, sharedSegments:1}
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(50.0, 100.0), pt(150.0, 100.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        let stats = route_set_stats(&rbd);
        assert_eq!(stats.shared_segments, 1);
        assert_eq!(stats.repeated_crossings, 0);
    }

    #[test]
    fn route_set_stats_no_overlap() {
        // Node: different y lines → {repeatedCrossings:0, sharedSegments:0}
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 200.0), pt(200.0, 200.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        let stats = route_set_stats(&rbd);
        assert_eq!(stats.shared_segments, 0);
        assert_eq!(stats.repeated_crossings, 0);
    }

    // -----------------------------------------------------------------------
    // routeSeparationScore / isBetterRouteSeparation
    // -----------------------------------------------------------------------

    #[test]
    fn route_separation_score_order() {
        // Node (confirmed above):
        // score({nextCloseCount:1,sharedSeg:0,repCross:0,pairClose:0,bends:2,dist:5})
        //   = [1,0,0,0,2,5]
        let stats = RouteSetStats { repeated_crossings: 0, shared_segments: 0 };
        let s1 = route_separation_score(1, &stats, 0, 2, 5.0);
        assert_eq!(s1, [1.0, 0.0, 0.0, 0.0, 2.0, 5.0]);
        let s2 = route_separation_score(0, &stats, 0, 3, 5.0);
        assert_eq!(s2, [0.0, 0.0, 0.0, 0.0, 3.0, 5.0]);
        // s2 beats s1 (lower closeCount)
        assert!(is_better_route_separation(&s2, Some(&s1)));
        assert!(!is_better_route_separation(&s1, Some(&s2)));
    }

    #[test]
    fn is_better_route_separation_vs_none() {
        // Node: any score beats None
        let stats = RouteSetStats { repeated_crossings: 0, shared_segments: 0 };
        let s = route_separation_score(0, &stats, 0, 0, 5.0);
        assert!(is_better_route_separation(&s, None));
    }

    #[test]
    fn is_better_route_separation_distance_tiebreak() {
        // Node: distance=7 score=[0,0,0,0,2,7] vs distance=5 score=[0,0,0,0,2,5]
        // → dist5 wins (smaller abs distance)
        let stats = RouteSetStats { repeated_crossings: 0, shared_segments: 0 };
        let s_dist7 = route_separation_score(0, &stats, 0, 2, 7.0);
        let s_dist5 = route_separation_score(0, &stats, 0, 2, 5.0);
        assert!(!is_better_route_separation(&s_dist7, Some(&s_dist5)));
        assert!(is_better_route_separation(&s_dist5, Some(&s_dist7)));
    }

    // -----------------------------------------------------------------------
    // routeSetScore / isBetterRouteSet
    // -----------------------------------------------------------------------

    #[test]
    fn route_set_score_basic() {
        // Node: two non-parallel routes → [0,0,0,totalBends]
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 200.0), pt(200.0, 200.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        let score = route_set_score(&rbd);
        assert_eq!(score[0], 0.0); // no close parallel
        assert_eq!(score[1], 0.0); // no shared segments
        assert_eq!(score[2], 0.0); // no repeated crossings
        assert_eq!(score[3], 0.0); // 0+0 bends
    }

    #[test]
    fn is_better_route_set_vs_none() {
        let score: [f64; 4] = [0.0, 0.0, 0.0, 2.0];
        assert!(is_better_route_set(&score, None));
    }

    #[test]
    fn is_better_route_set_lower_wins() {
        let s1: [f64; 4] = [1.0, 0.0, 0.0, 0.0];
        let s2: [f64; 4] = [0.0, 0.0, 0.0, 0.0];
        assert!(is_better_route_set(&s2, Some(&s1)));
        assert!(!is_better_route_set(&s1, Some(&s2)));
    }

    // -----------------------------------------------------------------------
    // separateCloseParallelRoutes — small fixture
    // -----------------------------------------------------------------------

    #[test]
    fn separate_close_parallel_routes_no_close_pair_unchanged() {
        // Node: routes far apart → separateCloseParallelRoutes returns them unchanged
        // Two routes at y=100 and y=200 (100px apart) with no closeness → immediately break.
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 200.0), pt(200.0, 200.0)]);
        let planned = vec![("r1".into(), r1.clone()), ("r2".into(), r2.clone())];
        let rels: Vec<SeparationRelationship> = vec![
            SeparationRelationship { id: "r1".into(), from: "A".into(), to: "B".into() },
            SeparationRelationship { id: "r2".into(), from: "C".into(), to: "D".into() },
        ];
        let node_rects: IndexMap<String, Rect> = IndexMap::new();
        let fixed_ports: IndexMap<String, bool> = IndexMap::new();
        let result = separate_close_parallel_routes(
            &planned, &rels, &node_rects, &fixed_ports, &NoopReroute,
        );
        // No close pair found → same routes returned.
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "r1");
        assert_eq!(result[1].0, "r2");
    }

    // -----------------------------------------------------------------------
    // alternateMiddleDoglegRoutes
    // -----------------------------------------------------------------------

    #[test]
    fn alternate_middle_dogleg_horizontal() {
        // Node (confirmed above):
        // route [{x:0,y:50},{x:82,y:50},{x:120,y:50},{x:120,y:80},{x:200,y:80}]
        // → horizontal dogleg → 2 alternatives
        // alt0 points: [{0,50},{82,50},{82,80},{120,80},{200,80}]
        // alt1 points: [{0,50},{82,50},{82,65},{120,65},{120,80},{200,80}]
        let route = orthogonal_route(vec![
            pt(0.0, 50.0), pt(82.0, 50.0), pt(120.0, 50.0), pt(120.0, 80.0), pt(200.0, 80.0),
        ]);
        let alts = alternate_middle_dogleg_routes(&route);
        assert_eq!(alts.len(), 2);
        // alt0: 5 points (sourceStub.x, targetStub.y as new middle)
        // After route_with_points simplification the 5-point route may simplify
        // if any consecutive points are collinear, but let's check the shape.
        // The new middle is {x:82,y:80} which connects {82,50}→{82,80}→{120,80} — that's
        // a valid L shape, so no simplification removes points.
        let alt0_pts = &alts[0].points;
        // {0,50}→{82,50}→{82,80}→{120,80}→{200,80}
        assert_eq!(alt0_pts[0], pt(0.0, 50.0));
        assert_eq!(alt0_pts[1], pt(82.0, 50.0));
        assert_eq!(alt0_pts[2], pt(82.0, 80.0));
        assert_eq!(alt0_pts[3], pt(120.0, 80.0));
        assert_eq!(alt0_pts[4], pt(200.0, 80.0));
        // alt1: 6 points with gutter_y=(50+80)/2=65
        let alt1_pts = &alts[1].points;
        assert_eq!(alt1_pts[0], pt(0.0, 50.0));
        assert_eq!(alt1_pts[1], pt(82.0, 50.0));
        assert_eq!(alt1_pts[2], pt(82.0, 65.0));
        assert_eq!(alt1_pts[3], pt(120.0, 65.0));
        assert_eq!(alt1_pts[4], pt(120.0, 80.0));
        assert_eq!(alt1_pts[5], pt(200.0, 80.0));
    }

    #[test]
    fn alternate_middle_dogleg_wrong_length_returns_empty() {
        // Node: route with != 5 points → []
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        assert!(alternate_middle_dogleg_routes(&route).is_empty());
    }

    #[test]
    fn alternate_middle_dogleg_vertical() {
        // Node: vertical dogleg
        // route [{x:50,y:0},{x:50,y:82},{x:100,y:82},{x:100,y:200},{x:80,y:200}]
        // → verticalEndpointDogleg: start.x(50)==sourceStub.x(50)==middleA.x(100)? NO
        // Let's construct a real vertical dogleg:
        // start.x == sourceStub.x == middleA.x AND middleA.y == targetStub.y AND targetStub.x == end.x
        // route [{x:50,y:0},{x:50,y:82},{x:50,y:120},{x:100,y:120},{x:100,y:200}]
        // start.x(50)==sourceStub.x(50)==middleA.x(50): ✓
        // middleA.y(120)==targetStub.y(120): ✓
        // targetStub.x(100)==end.x(100): ✓
        // sourceStub.y(82) != targetStub.y(120): ✓ (needed for gutterX)
        let route = orthogonal_route(vec![
            pt(50.0, 0.0), pt(50.0, 82.0), pt(50.0, 120.0), pt(100.0, 120.0), pt(100.0, 200.0),
        ]);
        let alts = alternate_middle_dogleg_routes(&route);
        assert_eq!(alts.len(), 2);
        // gutter_x = (50+100)/2 = 75
        let alt1_pts = &alts[1].points;
        assert_eq!(alt1_pts[2], pt(75.0, 82.0)); // {gutterX, sourceStub.y}
        assert_eq!(alt1_pts[3], pt(75.0, 120.0)); // {gutterX, targetStub.y}
    }

    // -----------------------------------------------------------------------
    // spreadUnitSlots — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn spread_unit_slots_zero_half_widths_reduces_to_even_spread() {
        // Node: spreadUnitSlots([0,0,0], 100) = [-25, 0, 25]
        let slots = spread_unit_slots(&[0.0, 0.0, 0.0], 100.0);
        assert_eq!(slots.len(), 3);
        assert!((slots[0] - -25.0).abs() < 1e-9);
        assert!((slots[1] - 0.0).abs() < 1e-9);
        assert!((slots[2] - 25.0).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_single_unit_zero_width_centres() {
        // Node: spreadUnitSlots([0], 100) = [0]  (single unit → centre = 0)
        // With hw=0: content=0, slack=100, gap=100/2=50, cursor=-50+50+0=0
        let slots = spread_unit_slots(&[0.0], 100.0);
        assert_eq!(slots.len(), 1);
        assert!((slots[0] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_nonzero_half_widths() {
        // Node: spreadUnitSlots([6,6,6], 54) = [-16.5, 0, 16.5]
        let slots = spread_unit_slots(&[6.0, 6.0, 6.0], 54.0);
        assert_eq!(slots.len(), 3);
        assert!((slots[0] - -16.5).abs() < 1e-9);
        assert!((slots[1] - 0.0).abs() < 1e-9);
        assert!((slots[2] - 16.5).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_no_slack_falls_back_to_even_centres() {
        // Node: spreadUnitSlots([6,6], 20) → slack=20-24=-4 ≤ 0 → fallback
        // fallback: [1/(2+1)-0.5, 2/(2+1)-0.5] * 20 = [-3.333..., 3.333...]
        let slots = spread_unit_slots(&[6.0, 6.0], 20.0);
        assert_eq!(slots.len(), 2);
        let expected0 = (1.0_f64 / 3.0 - 0.5) * 20.0;
        let expected1 = (2.0_f64 / 3.0 - 0.5) * 20.0;
        assert!((slots[0] - expected0).abs() < 1e-9);
        assert!((slots[1] - expected1).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_two_units_with_widths() {
        // Node: spreadUnitSlots([5,5], 100) = [-18.333..., 18.333...]
        let slots = spread_unit_slots(&[5.0, 5.0], 100.0);
        assert_eq!(slots.len(), 2);
        assert!((slots[0] - -18.333_333_333_333_332).abs() < 1e-9);
        assert!((slots[1] - 18.333_333_333_333_336).abs() < 1e-9);
    }
}
