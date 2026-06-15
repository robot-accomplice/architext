//! Route construction and C1 cleanup helpers.
//!
//! `EndpointSideUsage`, `recentered_*`, C1 input types (`RelationshipC1`,
//! `RouteInputC1`), `collapse_aligned_opposing_surface_route`,
//! `aligned_fixed_port_route`, `shared_segment_count`,
//! `non_endpoint_node_collision_count`, `recentered_without_new_shared_segments`,
//! `route_with_fewest_shared_segments`, `route_with_best_cleanup_candidate`,
//! `aligned_facing_endpoint_route`, `endpoint_stub_route`,
//! `route_with_endpoint_stubs`, `PlanRelationship`, `enforce_endpoint_stubs`.

use indexmap::IndexMap;

use crate::model::{Point, Rect};
use crate::route_constants::rect_center;
use crate::route_ports::{surface_capacity, PORT_STUB};

use super::helpers::{
    axis_aligned_segments, endpoint_offset_points, endpoint_side, offset_endpoint_route,
    route_collides_with_non_endpoints, route_with_points, shared_segment_length, side_endpoint_key,
};
use super::types::{Relationship, RouteData, RouteInput};

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
    pub(super) fn is_fixed_ports(&self, node_id: &str) -> bool {
        self.fixed_ports
            .and_then(|m| m.get(node_id).copied())
            .unwrap_or(false)
    }

    pub(super) fn as_route_input(&self) -> RouteInput<'a> {
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
    use super::helpers::route_intersects_rect_pub;
    let mut count = 0usize;
    for node_id in input.visible_node_ids {
        if node_id == relationship.from || node_id == relationship.to {
            continue;
        }
        if let Some(rect) = input.node_rects.get(node_id.as_str()) {
            if route_intersects_rect_pub(route, rect, 0.0) {
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

    use crate::route_ports::side_vector;
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
