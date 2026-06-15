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
}
