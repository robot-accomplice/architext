//! Geometry helpers: endpoint detection, axis-aligned segments, collision checks,
//! route-point construction, offset/polyline utilities, and shared-segment rendering.
//!
//! Covers Pass A (routeMountModel dependency surface) and the shared-segment /
//! render helpers at the top of routeEdges.js (Pass C1 L54–L129).

use crate::js_compat::js_number_to_string;
use crate::model::{Point, Rect};
use crate::route_geometry::{
    bend_count, bounds_for_points, line_samples, sample_cubic, sample_line,
    segment_intersects_rect,
};
use crate::route_labels::{with_readable_label, RouteForLabel};
use crate::route_ports::{anchor_for, anchor_for_with_overrides, port_for, SideAnchors};
use crate::route_rendering::{path_to_svg_with_hops, simplify_orthogonal_points, RouteRef};

use super::types::{AxisAlignedSegment, Relationship, RouteData, RouteInput};

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
pub(super) fn build_ml_path(points: &[Point]) -> String {
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
// recenteredEndpointPointsWithAnchors
// ---------------------------------------------------------------------------

/// Centering variant of `endpoint_offset_points` (rawOffset == 0) that honours
/// `sideAnchors` on the node rect (e.g. diamond tips for `decision:*` nodes).
///
/// JS `recenteredEndpointPoints` → `endpointOffsetPoints(…, 0)` → `anchorFor(rect, side)`
/// where `anchorFor` checks `rect.sideAnchors` first.  The Rust `endpoint_offset_points`
/// takes a plain `&Rect` (no sideAnchors), so this variant accepts the overrides
/// separately and calls `anchor_for_with_overrides` instead.
pub fn recentered_endpoint_points_with_anchors(
    points: &[Point],
    endpoint_index: usize,
    rect: &Rect,
    side: &str,
    side_anchors: Option<&SideAnchors>,
) -> Vec<Point> {
    let mut next: Vec<Point> = points.to_vec();
    let ep_idx = if endpoint_index == 0 { 0 } else { next.len() - 1 };
    let old_anchor = next[ep_idx].clone();
    let anchor = anchor_for_with_overrides(rect, side, side_anchors);
    next[ep_idx] = anchor.clone();
    let adjacent_idx = if ep_idx == 0 { 1 } else { next.len() - 2 };

    // 2-point case: insert port + elbow to keep orthogonality.
    if next.len() == 2 {
        let adjacent = next[adjacent_idx].clone();
        let port = port_for(rect, side, 18.0, 0.0, false).port;
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

    // Multi-point centering pass.
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
    next
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
// route_intersects_rect — pub(super) re-export for construction.rs
// ---------------------------------------------------------------------------

/// Re-export of the private `route_intersects_rect` for use by sibling submodules.
pub(super) fn route_intersects_rect_pub(route: &RouteData, rect: &Rect, padding: f64) -> bool {
    route_intersects_rect(route, rect, padding)
}

// ---------------------------------------------------------------------------
// sideEndpointKey (L108)
// ---------------------------------------------------------------------------

/// Port of JS `sideEndpointKey(nodeId, side)`.
///
/// Returns `"${nodeId} ${side}"` — a NUL-separated composite key matching
/// the JS template literal that uses the U+0000 byte as the separator between
/// `nodeId` and `side`.
pub fn side_endpoint_key(node_id: &str, side: &str) -> String {
    let mut key = String::with_capacity(node_id.len() + 1 + side.len());
    key.push_str(node_id);
    key.push('\0');
    key.push_str(side);
    key
}
