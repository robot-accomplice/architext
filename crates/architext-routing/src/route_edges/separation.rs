//! Close-parallel route separation (Pass C2) and spread-unit-slot distribution
//! (Pass C3 helper).
//!
//! `RouteSegment`, `axis_aligned_route_segments`, `CloseParallelPair`,
//! `close_parallel_segment_pair`, shifted-route helpers, `route_pair_index`,
//! `route_endpoints_are_perpendicular`, `close_parallel_run_count_*`,
//! `ROUTE_SEPARATION_DISTANCES`, `crossing_pair_key`, `RouteSetStats`,
//! `route_set_stats`, `route_separation_score`, `is_better_route_separation`,
//! `total_bends_for_routes`, `route_set_score`, `is_better_route_set`,
//! `SeparationRelationship`, `RerouteCallback`, `NoopReroute`,
//! `separate_close_parallel_routes`, `alternate_middle_dogleg_routes`,
//! `spread_unit_slots`.

use indexmap::IndexMap;

use crate::model::{Point, Rect};
use crate::route_constants::rect_center;

use super::construction::route_with_endpoint_stubs;
use super::helpers::{
    endpoint_side, offset_endpoint_route, route_collides_with_non_endpoints,
    route_has_endpoint_traversal,
};
use super::types::{Relationship, RouteData, RouteInput};

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
    Some(super::helpers::route_with_points(route, points, route.controls.clone()))
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
                    // visible_node_ids must use node IDs (from node_rects), NOT route/edge IDs.
                    // JS passes input.visibleNodeIds which is the list of diagram node IDs.
                    {
                        let simple_rel = Relationship { from: rel.from.as_str(), to: rel.to.as_str() };
                        let visible_node_ids: Vec<String> = node_rects.keys().cloned().collect();
                        let simple_input = RouteInput {
                            visible_node_ids: &visible_node_ids,
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
        let alt1 = super::helpers::route_with_points(
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
        let alt2 = super::helpers::route_with_points(
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
        let alt1 = super::helpers::route_with_points(
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
        let alt2 = super::helpers::route_with_points(
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
