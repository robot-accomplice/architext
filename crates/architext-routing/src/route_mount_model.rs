//! Faithful port of `viewer/src/routing/routeMountModel.js` (1005 loc).
//!
//! Pass B of the Phase 1B routeEdges rewrite. Every exported function the
//! orchestration layer calls is reproduced here. Functions that depend on
//! `buildRouteForSides` (a JS callback) accept an `Option<&dyn BuildRouteForSides>`
//! trait object — callers that do not yet have that wired pass `None` and the
//! functions no-op exactly as the JS `if (!buildRouteForSides) return` guards do.
//!
//! Parity decisions:
//! - JS `Map` iterated for ordering decisions → `IndexMap` (insertion-order preserved).
//! - `Math.hypot` → `crate::js_compat::js_hypot` (libm, bit-identical native+wasm).
//! - `Math.round` is not used by this module; js_round imported for completeness.
//! - `String.localeCompare` / `.sort()` on ids → `js_locale_compare`.
//! - Guard-revert: every optimiser move snapshots before, restores if cost ≥ before.
//!   "strictly lowers" = `<`, never `<=`. This is parity-critical.
//! - `route.points.at(-1)` → `.last()` — same semantics on non-empty vec.
//! - JS sparse-object field access (`route.bends ?? 0`) → `.unwrap_or(0)`.
//! - `surfacesOf` iteration order matches JS Map insertion order via IndexMap.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::js_compat::{js_hypot, js_locale_compare};
use crate::model::{Point, Rect};
use crate::route_constants::{
    rect_center, BRIDGE_GUTTER_CLEARANCE, BRIDGE_LANE_GAP, BRIDGE_MAX_LANES,
    BRIDGE_MOUNT_OFFSET, MIN_LEGIBLE_GAP, MOUNT_COST, MOUNT_MAX_ITERS,
    RECIPROCAL_PARALLEL_OFFSET,
};
use crate::route_edges::{
    axis_aligned_segments, endpoint_side, endpoint_spread_offset, offset_endpoint_route,
    offset_orthogonal_polyline, route_collides_with_non_endpoints, route_has_endpoint_traversal,
    route_with_points, shared_segment_length, side_needs_post_selection_centering,
    RouteData, RouteInput, Relationship,
};
use crate::route_geometry::shallow_jog_count;
use crate::route_intent::{
    derive_route_intent, semantic_surface_options, DeriveRouteIntentInput, IntentRelationship,
    SemanticSurfaceOptionsInput, SidePair,
};
use crate::route_ports::surface_capacity;

// ---------------------------------------------------------------------------
// Input types for this module
// ---------------------------------------------------------------------------

/// Richer `input` object routeMountModel functions need beyond the minimal
/// RouteInput used in route_edges.rs.
pub struct MountInput<'a> {
    pub visible_node_ids: &'a [String],
    pub node_rects: &'a IndexMap<String, MountRect>,
    pub lane_index_by_node: &'a IndexMap<String, i64>,
    pub row_index_by_node: &'a IndexMap<String, i64>,
    pub canvas_width: f64,
    pub canvas_height: f64,
}

/// A node rect with the optional `fixedPorts` flag from the JS input.
#[derive(Debug, Clone)]
pub struct MountRect {
    pub rect: Rect,
    /// JS `rect.fixedPorts` — when true the optimiser must not re-home endpoints.
    pub fixed_ports: bool,
}

/// A relationship descriptor as seen by the mount model.
#[derive(Debug, Clone)]
pub struct MountRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relationship_type: String,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
    pub display_index: i64,
    // Fields forwarded to route_intent functions:
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
}

/// Callback interface that replaces the JS `buildRouteForSides(rel, startSide, endSide, routeById)`
/// parameter. The orchestration layer wires this up; the mount model calls it without knowing
/// the implementation. Returns `None` when the requested sides cannot be routed.
pub trait BuildRouteForSides {
    fn build(
        &self,
        rel: &MountRelationship,
        start_side: &str,
        end_side: &str,
        route_by_id: &IndexMap<String, RouteData>,
    ) -> Option<RouteData>;
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn point_key(p: &Point) -> String {
    format!("{},{}", p.x, p.y)
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

const SIDE_NORMAL: [(&str, Point); 4] = [
    ("top",    Point { x: 0.0, y: -1.0 }),
    ("bottom", Point { x: 0.0, y:  1.0 }),
    ("left",   Point { x: -1.0, y: 0.0 }),
    ("right",  Point { x:  1.0, y: 0.0 }),
];

fn side_normal(side: &str) -> Option<&'static Point> {
    SIDE_NORMAL.iter().find(|(s, _)| *s == side).map(|(_, n)| n)
}

/// JS `isStraightFacing(route)`
fn is_straight_facing(route: &RouteData) -> bool {
    if route.points.len() != 2 {
        return false;
    }
    let a = &route.points[0];
    let b = &route.points[1];
    a.x == b.x || a.y == b.y
}

/// Deterministic deep clone of routeById for trial/accept.
fn snapshot_routes(route_by_id: &IndexMap<String, RouteData>) -> IndexMap<String, RouteData> {
    route_by_id
        .iter()
        .map(|(id, r)| {
            (
                id.clone(),
                RouteData {
                    d: r.d.clone(),
                    points: r.points.iter().map(|p| Point { x: p.x, y: p.y }).collect(),
                    controls: r.controls.clone(),
                    samples: r.samples.iter().map(|p| Point { x: p.x, y: p.y }).collect(),
                    sample_bounds: r.sample_bounds.clone(),
                    bends: r.bends,
                    label_x: r.label_x,
                    label_y: r.label_y,
                    style: r.style.clone(),
                    extra: r.extra.clone(),
                },
            )
        })
        .collect()
}

/// Restore all routes from a snapshot.
fn restore_routes(route_by_id: &mut IndexMap<String, RouteData>, saved: &IndexMap<String, RouteData>) {
    for (id, route) in saved {
        route_by_id.insert(id.clone(), route.clone());
    }
}

// extract_rects creates a temporary IndexMap<String, Rect> for RouteInput use.
fn extract_rects(node_rects: &IndexMap<String, MountRect>) -> IndexMap<String, Rect> {
    node_rects
        .iter()
        .map(|(k, v)| (k.clone(), v.rect.clone()))
        .collect()
}

fn make_route_input<'a>(
    visible_node_ids: &'a [String],
    rects: &'a IndexMap<String, Rect>,
) -> RouteInput<'a> {
    RouteInput {
        visible_node_ids,
        node_rects: rects,
    }
}

fn make_relationship<'a>(rel: &'a MountRelationship) -> Relationship<'a> {
    Relationship {
        from: &rel.from,
        to: &rel.to,
    }
}

fn make_intent_relationship(rel: &MountRelationship) -> IntentRelationship {
    IntentRelationship {
        id: rel.id.clone(),
        kind: rel.kind.clone(),
        return_of: rel.return_of.clone(),
        outcome: rel.outcome.clone(),
        relationship_type: Some(rel.relationship_type.clone()),
        step_id: rel.step_id.clone(),
        flow_id: rel.flow_id.clone(),
        preferred_start_side: rel.preferred_start_side.clone(),
        preferred_end_side: rel.preferred_end_side.clone(),
    }
}

// ---------------------------------------------------------------------------
// routeLength
// ---------------------------------------------------------------------------

/// Total wire length (Manhattan for orthogonal routes). Not exported (private helper).
fn route_length(route: &RouteData) -> f64 {
    let mut total = 0.0f64;
    for i in 0..route.points.len().saturating_sub(1) {
        let dx = route.points[i + 1].x - route.points[i].x;
        let dy = route.points[i + 1].y - route.points[i].y;
        total += js_hypot(dx, dy);
    }
    total
}

// ---------------------------------------------------------------------------
// nodeGapLength
// ---------------------------------------------------------------------------

/// Shortest possible wire between two nodes: the Manhattan gap between bounding
/// boxes (0 on axes where they overlap).
fn node_gap_length(from_rect: Option<&Rect>, to_rect: Option<&Rect>) -> f64 {
    let (fr, tr) = match (from_rect, to_rect) {
        (Some(a), Some(b)) => (a, b),
        _ => return 0.0,
    };
    let gap_x = f64::max(
        0.0,
        f64::max(fr.x - (tr.x + tr.width), tr.x - (fr.x + fr.width)),
    );
    let gap_y = f64::max(
        0.0,
        f64::max(fr.y - (tr.y + tr.height), tr.y - (fr.y + fr.height)),
    );
    gap_x + gap_y
}

// ---------------------------------------------------------------------------
// excessLength
// ---------------------------------------------------------------------------

/// Port of JS `excessLength(route, fromRect, toRect)`.
///
/// Avoidable detour only (raw length minus the irreducible node gap).
pub fn excess_length(route: &RouteData, from_rect: Option<&Rect>, to_rect: Option<&Rect>) -> f64 {
    if route.points.is_empty() {
        return 0.0;
    }
    f64::max(0.0, route_length(route) - node_gap_length(from_rect, to_rect))
}

// ---------------------------------------------------------------------------
// doglegCount
// ---------------------------------------------------------------------------

/// Port of JS `doglegCount(route, fromRect, toRect)`.
///
/// Counts segments travelling against the from→to direction.
pub fn dogleg_count(route: &RouteData, from_rect: Option<&Rect>, to_rect: Option<&Rect>) -> f64 {
    let (fr, tr) = match (from_rect, to_rect) {
        (Some(a), Some(b)) => (a, b),
        _ => return 0.0,
    };
    if route.points.is_empty() {
        return 0.0;
    }
    let from = rect_center(fr);
    let to = rect_center(tr);
    // JS Math.sign returns 0 for 0; use manual sign to match JS exactly.
    let js_sign_f = |v: f64| -> i32 {
        if v > 0.0 { 1 } else if v < 0.0 { -1 } else { 0 }
    };
    let x_dir = js_sign_f(to.x - from.x);
    let y_dir = js_sign_f(to.y - from.y);
    let mut count = 0.0f64;
    for i in 0..route.points.len().saturating_sub(1) {
        let dx = route.points[i + 1].x - route.points[i].x;
        let dy = route.points[i + 1].y - route.points[i].y;
        if x_dir != 0 && js_sign_f(dx) == -x_dir {
            count += 1.0;
        }
        if y_dir != 0 && js_sign_f(dy) == -y_dir {
            count += 1.0;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// intentMismatchCount
// ---------------------------------------------------------------------------

/// Port of JS `intentMismatchCount(route, relationship, input)`.
pub fn intent_mismatch_count(
    route: &RouteData,
    relationship: &MountRelationship,
    node_rects: &IndexMap<String, MountRect>,
) -> f64 {
    if route.points.is_empty() {
        return 0.0;
    }
    let mut count = 0.0f64;
    let endpoints = [
        (&relationship.from, 0usize, &relationship.to),
        (&relationship.to, usize::MAX, &relationship.from),
    ];
    for (node_id, ep_index, opposite_id) in &endpoints {
        let mr = match node_rects.get(*node_id) {
            Some(r) => r,
            None => continue,
        };
        let opp = match node_rects.get(*opposite_id) {
            Some(r) => r,
            None => continue,
        };
        let point = if *ep_index == 0 {
            &route.points[0]
        } else {
            route.points.last().unwrap()
        };
        let side = endpoint_side(&mr.rect, point);
        let normal = match side_normal(side) {
            Some(n) => n,
            None => continue,
        };
        let c = rect_center(&mr.rect);
        let o = rect_center(&opp.rect);
        if normal.x * (o.x - c.x) + normal.y * (o.y - c.y) < 0.0 {
            count += 1.0;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// routeIntersections
// ---------------------------------------------------------------------------

/// Port of JS `routeIntersections(routeA, routeB)`.
///
/// Counts distinct intersection points (including T-junctions), excluding
/// shared mounts.
pub fn route_intersections(route_a: &RouteData, route_b: &RouteData) -> usize {
    let segs_a = axis_aligned_segments(route_a);
    let segs_b = axis_aligned_segments(route_b);
    let terminal_a: HashSet<String> = [&route_a.points[0], route_a.points.last().unwrap()]
        .iter()
        .map(|p| point_key(p))
        .collect();
    let terminal_b: HashSet<String> = [&route_b.points[0], route_b.points.last().unwrap()]
        .iter()
        .map(|p| point_key(p))
        .collect();
    let mut points: HashSet<String> = HashSet::new();
    for left in &segs_a {
        for right in &segs_b {
            if left.orientation == right.orientation {
                continue;
            }
            let (h, v) = if left.orientation == "horizontal" {
                (left, right)
            } else {
                (right, left)
            };
            if v.line >= h.min && v.line <= h.max && h.line >= v.min && h.line <= v.max {
                let key = format!("{},{}", v.line, h.line);
                if terminal_a.contains(&key) && terminal_b.contains(&key) {
                    continue; // shared mount
                }
                points.insert(key);
            }
        }
    }
    points.len()
}

// ---------------------------------------------------------------------------
// strictCrossingCount
// ---------------------------------------------------------------------------

/// Port of JS `strictCrossingCount(routeA, routeB)`.
///
/// Only true X-crossings (strictly interior intersection), not T-junctions.
pub fn strict_crossing_count(route_a: &RouteData, route_b: &RouteData) -> f64 {
    let segs_a = axis_aligned_segments(route_a);
    let segs_b = axis_aligned_segments(route_b);
    let mut count = 0.0f64;
    for left in &segs_a {
        for right in &segs_b {
            if left.orientation == right.orientation {
                continue;
            }
            let (h, v) = if left.orientation == "horizontal" {
                (left, right)
            } else {
                (right, left)
            };
            if v.line > h.min && v.line < h.max && h.line > v.min && h.line < v.max {
                count += 1.0;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// surfaceCrampedUnits
// ---------------------------------------------------------------------------

/// Port of JS `surfaceCrampedUnits(positions, length)`.
///
/// Raw crowding magnitude: sum of shortfalls below MIN_LEGIBLE_GAP.
pub fn surface_cramped_units(positions: &[f64], length: f64) -> f64 {
    let mut sorted = positions.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut units = 0.0f64;
    // guards = [0, ...sorted, length]
    let guards_len = sorted.len() + 2;
    let guard = |i: usize| -> f64 {
        if i == 0 {
            0.0
        } else if i == guards_len - 1 {
            length
        } else {
            sorted[i - 1]
        }
    };
    for i in 0..guards_len - 1 {
        let gap = guard(i + 1) - guard(i);
        if gap < MIN_LEGIBLE_GAP {
            units += MIN_LEGIBLE_GAP - gap;
        }
    }
    units
}

// ---------------------------------------------------------------------------
// surfaceSpacingCost
// ---------------------------------------------------------------------------

/// Port of JS `surfaceSpacingCost(positions, length)`.
pub fn surface_spacing_cost(positions: &[f64], length: f64) -> f64 {
    surface_cramped_units(positions, length) * MOUNT_COST.cramped
}

// ---------------------------------------------------------------------------
// Surface descriptor (returned by surfacesOf)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SurfaceInfo {
    pub rect: Rect,
    pub side: String,
    pub positions: Vec<f64>,
}

// ---------------------------------------------------------------------------
// movableEndpoints (private helper)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct EndpointDescriptor {
    id: String,
    endpoint_index: usize, // 0 = first, usize::MAX = last
    node_id: String,
    side: String,
    rect: Rect,
    point: Point,
}

fn movable_endpoints(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) -> Vec<EndpointDescriptor> {
    let mut out = Vec::new();
    for (id, route) in route_by_id {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        let endpoints = [
            (&rel.from, 0usize),
            (&rel.to, usize::MAX),
        ];
        for (node_id, ep_index) in &endpoints {
            let mr = match node_rects.get(*node_id) {
                Some(r) => r,
                None => continue,
            };
            if mr.fixed_ports {
                continue;
            }
            let point = if *ep_index == 0 {
                route.points[0].clone()
            } else {
                route.points.last().unwrap().clone()
            };
            let side = endpoint_side(&mr.rect, &point);
            if !side_needs_post_selection_centering(side) {
                continue;
            }
            out.push(EndpointDescriptor {
                id: id.clone(),
                endpoint_index: *ep_index,
                node_id: (*node_id).clone(),
                side: side.to_string(),
                rect: mr.rect.clone(),
                point,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// surfacesOf
// ---------------------------------------------------------------------------

/// Port of JS `surfacesOf(routeById, relationshipById, input)`.
///
/// Returns an `IndexMap<String, SurfaceInfo>` keyed by `"${nodeId} ${side}"`.
/// Insertion order matches JS Map insertion order (which the cramped/capacity
/// scoring passes iterate).
pub fn surfaces_of(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) -> IndexMap<String, SurfaceInfo> {
    let mut surfaces: IndexMap<String, SurfaceInfo> = IndexMap::new();
    for ep in movable_endpoints(route_by_id, rel_by_id, node_rects) {
        let key = format!("{} {}", ep.node_id, ep.side);
        let axis_start = if ep.side == "left" || ep.side == "right" {
            ep.rect.y
        } else {
            ep.rect.x
        };
        let pos = if ep.side == "left" || ep.side == "right" {
            ep.point.y
        } else {
            ep.point.x
        } - axis_start;
        if !surfaces.contains_key(&key) {
            surfaces.insert(
                key.clone(),
                SurfaceInfo {
                    rect: ep.rect.clone(),
                    side: ep.side.clone(),
                    positions: Vec::new(),
                },
            );
        }
        surfaces.get_mut(&key).unwrap().positions.push(pos);
    }
    surfaces
}

// ---------------------------------------------------------------------------
// weightedMountCost (private)
// ---------------------------------------------------------------------------

fn weighted_mount_cost(factors: &MountCostFactors) -> f64 {
    let mc = &MOUNT_COST;
    mc.collision * factors.collision
        + mc.endpoint_traversal * factors.endpoint_traversal
        + mc.repeated_crossing * factors.repeated_crossing
        + mc.self_overlap * factors.self_overlap
        + mc.shared_segment * factors.shared_segment
        + mc.shared_segment_length * factors.shared_segment_length
        + mc.perimeter_fallback * factors.perimeter_fallback
        + mc.crossing * factors.crossing
        + mc.monotonic_backtrack * factors.monotonic_backtrack
        + mc.bend * factors.bend
        + mc.dogleg * factors.dogleg
        + mc.shallow_jog * factors.shallow_jog
        + mc.cramped * factors.cramped
        + mc.intent_mismatch * factors.intent_mismatch
        + mc.length * factors.length
        + mc.over_capacity * factors.over_capacity
}

// ---------------------------------------------------------------------------
// MountCostFactors
// ---------------------------------------------------------------------------

/// Raw factor breakdown, mirrors JS `factors` object in `mountCostFactors`.
#[derive(Debug, Clone, Default)]
pub struct MountCostFactors {
    pub collision: f64,
    pub endpoint_traversal: f64,
    pub repeated_crossing: f64,
    pub self_overlap: f64,
    pub shared_segment: f64,
    pub shared_segment_length: f64,
    pub perimeter_fallback: f64,
    pub crossing: f64,
    pub monotonic_backtrack: f64,
    pub bend: f64,
    pub dogleg: f64,
    pub shallow_jog: f64,
    pub cramped: f64,
    pub intent_mismatch: f64,
    pub length: f64,
    pub over_capacity: f64,
}

// ---------------------------------------------------------------------------
// mountCostFactors
// ---------------------------------------------------------------------------

/// Port of JS `mountCostFactors(routeById, relationshipById, input)`.
pub fn mount_cost_factors(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) -> MountCostFactors {
    let rects = extract_rects(input.node_rects);
    let ri = make_route_input(input.visible_node_ids, &rects);

    let mut f = MountCostFactors::default();
    let routes: Vec<(&String, &RouteData)> = route_by_id.iter().collect();

    for (id, route) in &routes {
        let rel = match rel_by_id.get(*id) {
            Some(r) => r,
            None => continue,
        };
        let edge_rel = make_relationship(rel);
        if route_collides_with_non_endpoints(route, &edge_rel, &ri) {
            f.collision += 1.0;
        }
        if route_has_endpoint_traversal(route, &edge_rel, &ri) {
            f.endpoint_traversal += 1.0;
        }
        f.bend += route.bends as f64;
        f.shallow_jog += shallow_jog_count(&route.points) as f64;
        // repeatedCrossings / selfOverlappingSegments come from extra JSON fields
        f.repeated_crossing += route
            .extra
            .get("repeatedCrossings")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        f.self_overlap += route
            .extra
            .get("selfOverlappingSegments")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        // perimeterFallbackCost / monotonicBacktrackCost nested in qualityCosts
        let quality_costs = route.extra.get("qualityCosts");
        if quality_costs
            .and_then(|qc| qc.get("perimeterFallbackCost"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            > 0.0
        {
            f.perimeter_fallback += 1.0;
        }
        if quality_costs
            .and_then(|qc| qc.get("monotonicBacktrackCost"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            > 0.0
        {
            f.monotonic_backtrack += 1.0;
        }
        let from_rect = input.node_rects.get(&rel.from).map(|mr| &mr.rect);
        let to_rect = input.node_rects.get(&rel.to).map(|mr| &mr.rect);
        f.length += excess_length(route, from_rect, to_rect);
        f.dogleg += dogleg_count(route, from_rect, to_rect);
        f.intent_mismatch += intent_mismatch_count(route, rel, input.node_rects);
    }

    // Pairwise crossing + shared-segment
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            f.crossing += strict_crossing_count(routes[i].1, routes[j].1);
            let segs_a = axis_aligned_segments(routes[i].1);
            let segs_b = axis_aligned_segments(routes[j].1);
            for l in &segs_a {
                for r in &segs_b {
                    let len = shared_segment_length(l, r);
                    if len > 1.0 {
                        f.shared_segment += 1.0;
                        f.shared_segment_length += len;
                    }
                }
            }
        }
    }

    // Surface-level factors
    for surface in surfaces_of(route_by_id, rel_by_id, input.node_rects).values() {
        let length = if surface.side == "left" || surface.side == "right" {
            surface.rect.height
        } else {
            surface.rect.width
        };
        let cap = surface_capacity(&surface.rect, &surface.side) as f64;
        f.over_capacity += f64::max(0.0, surface.positions.len() as f64 - cap);
        f.cramped += surface_cramped_units(&surface.positions, length);
    }

    f
}

// ---------------------------------------------------------------------------
// mountAssignmentCost
// ---------------------------------------------------------------------------

/// Port of JS `mountAssignmentCost(routeById, relationshipById, input)`.
pub fn mount_assignment_cost(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) -> f64 {
    weighted_mount_cost(&mount_cost_factors(route_by_id, rel_by_id, input))
}

// ---------------------------------------------------------------------------
// applyOffsetWithMatch
// ---------------------------------------------------------------------------

/// Descriptor for a movable endpoint (subset of EndpointDescriptor, public).
pub struct MountTarget {
    pub id: String,
    pub endpoint_index: usize, // 0 = first, usize::MAX = last
    pub side: String,
    pub rect: Rect,
}

/// Port of JS `applyOffsetWithMatch(routeById, relationshipById, input, target, delta)`.
pub fn apply_offset_with_match(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
    target: &MountTarget,
    delta: f64,
) {
    let route = match route_by_id.get(&target.id) {
        Some(r) => r.clone(),
        None => return,
    };
    let rel = match rel_by_id.get(&target.id) {
        Some(r) => r,
        None => return,
    };
    let straight_facing = is_straight_facing(&route);
    let axis = if target.side == "left" || target.side == "right" {
        "y"
    } else {
        "x"
    };
    let center = if axis == "y" {
        target.rect.y + target.rect.height / 2.0
    } else {
        target.rect.x + target.rect.width / 2.0
    };
    let point = if target.endpoint_index == 0 {
        &route.points[0]
    } else {
        route.points.last().unwrap()
    };
    let point_axis = if axis == "y" { point.y } else { point.x };
    let raw_offset = point_axis - center + delta;
    let mut moved = offset_endpoint_route(&route, target.endpoint_index, &target.rect, &target.side, raw_offset);
    route_by_id.insert(target.id.clone(), moved.clone());

    if !straight_facing {
        return;
    }

    // Co-shift the partner endpoint so the straight facing edge stays straight.
    let partner_index = if target.endpoint_index == 0 {
        usize::MAX
    } else {
        0
    };
    let partner_node_id = if target.endpoint_index == 0 {
        &rel.to
    } else {
        &rel.from
    };
    let partner_mr = match node_rects.get(partner_node_id) {
        Some(r) => r,
        None => return,
    };
    let partner_point = if partner_index == 0 {
        moved.points[0].clone()
    } else {
        moved.points.last().unwrap().clone()
    };
    let partner_side = endpoint_side(&partner_mr.rect, &partner_point);
    let partner_center = if axis == "y" {
        partner_mr.rect.y + partner_mr.rect.height / 2.0
    } else {
        partner_mr.rect.x + partner_mr.rect.width / 2.0
    };
    let partner_axis = if axis == "y" { partner_point.y } else { partner_point.x };
    let partner_offset = partner_axis - partner_center + delta;
    moved = offset_endpoint_route(&moved, partner_index, &partner_mr.rect, partner_side, partner_offset);
    route_by_id.insert(target.id.clone(), moved);
}

// ---------------------------------------------------------------------------
// surfaceEndpointGroups (private)
// ---------------------------------------------------------------------------

struct SurfaceEndpoint {
    id: String,
    endpoint_index: usize,
    rect: Rect,
    side: String,
    opp_centre: f64,
    display_index: i64,
}

fn surface_endpoint_groups(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) -> IndexMap<String, Vec<SurfaceEndpoint>> {
    let mut groups: IndexMap<String, Vec<SurfaceEndpoint>> = IndexMap::new();
    for (id, route) in route_by_id {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        let endpoints = [
            (&rel.from, 0usize, &rel.to),
            (&rel.to, usize::MAX, &rel.from),
        ];
        for (node_id, ep_index, opposite_id) in &endpoints {
            let mr = match node_rects.get(*node_id) {
                Some(r) => r,
                None => continue,
            };
            if mr.fixed_ports {
                continue;
            }
            let point = if *ep_index == 0 {
                &route.points[0]
            } else {
                route.points.last().unwrap()
            };
            let side = endpoint_side(&mr.rect, point);
            if !side_needs_post_selection_centering(side) {
                continue;
            }
            let key = format!("{} {}", node_id, side);
            let opp = node_rects.get(*opposite_id);
            let axis = if side == "left" || side == "right" { "y" } else { "x" };
            let opp_centre = opp
                .map(|o| {
                    if axis == "y" {
                        o.rect.y + o.rect.height / 2.0
                    } else {
                        o.rect.x + o.rect.width / 2.0
                    }
                })
                .unwrap_or(0.0);
            if !groups.contains_key(&key) {
                groups.insert(key.clone(), Vec::new());
            }
            groups.get_mut(&key).unwrap().push(SurfaceEndpoint {
                id: id.clone(),
                endpoint_index: *ep_index,
                rect: mr.rect.clone(),
                side: side.to_string(),
                opp_centre,
                display_index: rel.display_index,
            });
        }
    }
    groups
}

// ---------------------------------------------------------------------------
// respreadSurfaces (private)
// ---------------------------------------------------------------------------

fn respread_surfaces(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) {
    // Collect groups first to avoid borrow issues.
    let groups = surface_endpoint_groups(route_by_id, rel_by_id, node_rects);
    for (_key, mut endpoints) in groups {
        if endpoints.len() < 2 {
            continue;
        }
        // JS: .sort((a,b) => a.oppCentre - b.oppCentre || a.displayIndex - b.displayIndex || a.id.localeCompare(b.id))
        endpoints.sort_by(|a, b| {
            a.opp_centre
                .partial_cmp(&b.opp_centre)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.display_index.cmp(&b.display_index))
                .then_with(|| js_locale_compare(&a.id, &b.id))
        });
        for (index, ep) in endpoints.iter().enumerate() {
            let route = match route_by_id.get(&ep.id) {
                Some(r) => r.clone(),
                None => continue,
            };
            let offset = endpoint_spread_offset(index, endpoints.len(), &ep.rect, &ep.side);
            let moved = offset_endpoint_route(&route, ep.endpoint_index, &ep.rect, &ep.side, offset);
            route_by_id.insert(ep.id.clone(), moved);
        }
    }
}

// ---------------------------------------------------------------------------
// trySideMoves (private)
// ---------------------------------------------------------------------------

fn try_side_moves(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    builder: Option<&dyn BuildRouteForSides>,
) {
    let builder = match builder {
        Some(b) => b,
        None => return,
    };
    let rects = extract_rects(input.node_rects);
    let ri = make_route_input(input.visible_node_ids, &rects);

    let ids: Vec<String> = {
        let mut v: Vec<String> = route_by_id.keys().cloned().collect();
        v.sort_by(|a, b| js_locale_compare(a, b));
        v
    };

    for id in &ids {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        let route = match route_by_id.get(id) {
            Some(r) => r.clone(),
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        // Only flow edges
        if rel.relationship_type != "flow" {
            continue;
        }
        // Respect fixed pins
        if rel.preferred_start_side.is_some() || rel.preferred_end_side.is_some() {
            continue;
        }
        let from_mr = match input.node_rects.get(&rel.from) {
            Some(r) => r,
            None => continue,
        };
        let to_mr = match input.node_rects.get(&rel.to) {
            Some(r) => r,
            None => continue,
        };
        if from_mr.fixed_ports || to_mr.fixed_ports {
            continue;
        }
        let start_side = endpoint_side(&from_mr.rect, &route.points[0]).to_string();
        let end_side = endpoint_side(&to_mr.rect, route.points.last().unwrap()).to_string();

        for candidate_start in SIDES {
            for candidate_end in SIDES {
                if candidate_start == start_side && candidate_end == end_side {
                    continue;
                }
                let before = mount_assignment_cost(route_by_id, rel_by_id, input);
                let saved = snapshot_routes(route_by_id);
                let rebuilt = builder.build(rel, candidate_start, candidate_end, route_by_id);
                let rebuilt = match rebuilt {
                    Some(r) => r,
                    None => continue,
                };
                let edge_rel = make_relationship(rel);
                if route_collides_with_non_endpoints(&rebuilt, &edge_rel, &ri) {
                    continue;
                }
                route_by_id.insert(id.clone(), rebuilt);
                respread_surfaces(route_by_id, rel_by_id, input.node_rects);
                if mount_assignment_cost(route_by_id, rel_by_id, input) >= before {
                    restore_routes(route_by_id, &saved);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// reliefCandidateIds (private)
// ---------------------------------------------------------------------------

fn relief_candidate_ids(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) -> Vec<String> {
    let surfs = surfaces_of(route_by_id, rel_by_id, node_rects);
    let mut over_capacity_surfaces: HashSet<String> = HashSet::new();
    for (key, surface) in &surfs {
        let cap = surface_capacity(&surface.rect, &surface.side) as usize;
        if surface.positions.len() > cap {
            over_capacity_surfaces.insert(key.clone());
        }
    }

    // Build directed set (from\0to strings) for reciprocal detection
    let mut directed: HashSet<String> = HashSet::new();
    for rel in rel_by_id.values() {
        if route_by_id.contains_key(&rel.id) {
            directed.insert(format!("{}\x00{}", rel.from, rel.to));
        }
    }

    let routes: Vec<(&String, &RouteData)> = route_by_id.iter().collect();
    let mut crossing: HashSet<String> = HashSet::new();
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            if route_intersections(routes[i].1, routes[j].1) > 0 {
                crossing.insert(routes[i].0.clone());
                crossing.insert(routes[j].0.clone());
            }
        }
    }

    let mut ids: Vec<String> = Vec::new();
    for (id, route) in route_by_id {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        let from_mr = node_rects.get(&rel.from);
        let to_mr = node_rects.get(&rel.to);
        let start_side = from_mr
            .map(|mr| endpoint_side(&mr.rect, &route.points[0]).to_string())
            .unwrap_or_default();
        let end_side = to_mr
            .map(|mr| endpoint_side(&mr.rect, route.points.last().unwrap()).to_string())
            .unwrap_or_default();
        let on_over_capacity = over_capacity_surfaces
            .contains(&format!("{} {}", rel.from, start_side))
            || over_capacity_surfaces.contains(&format!("{} {}", rel.to, end_side));
        let reciprocal_crossing =
            crossing.contains(id) && directed.contains(&format!("{}\x00{}", rel.to, rel.from));
        if on_over_capacity || reciprocal_crossing {
            ids.push(id.clone());
        }
    }
    ids.sort_by(|a, b| js_locale_compare(a, b));
    ids
}

// ---------------------------------------------------------------------------
// sideFacesPartner / idealFacingSide (private)
// ---------------------------------------------------------------------------

fn side_faces_partner(side: &str, rect: &Rect, partner_rect: &Rect) -> bool {
    let center = rect_center(rect);
    let partner = rect_center(partner_rect);
    let normal = match side_normal(side) {
        Some(n) => n,
        None => return false,
    };
    normal.x * (partner.x - center.x) + normal.y * (partner.y - center.y) > 0.0
}

fn ideal_facing_side(rect: &Rect, partner_rect: &Rect) -> &'static str {
    let center = rect_center(rect);
    let partner = rect_center(partner_rect);
    let dx = partner.x - center.x;
    let dy = partner.y - center.y;
    let mut best = SIDES[0];
    let mut best_dot = f64::NEG_INFINITY;
    for side in SIDES {
        let normal = match side_normal(side) {
            Some(n) => n,
            None => continue,
        };
        let dot = normal.x * dx + normal.y * dy;
        if dot > best_dot {
            best_dot = dot;
            best = side;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// reciprocalPairs (private)
// ---------------------------------------------------------------------------

fn reciprocal_pairs(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
) -> Vec<[String; 2]> {
    let mut by_pair: IndexMap<String, Vec<String>> = IndexMap::new();
    for rel in rel_by_id.values() {
        if !route_by_id.contains_key(&rel.id) {
            continue;
        }
        let mut nodes = [rel.from.clone(), rel.to.clone()];
        nodes.sort();
        let key = format!("{} {}", nodes[0], nodes[1]);
        by_pair.entry(key).or_default().push(rel.id.clone());
    }
    let mut pairs: Vec<[String; 2]> = Vec::new();
    for ids in by_pair.values() {
        if ids.len() == 2 {
            let mut sorted = ids.clone();
            sorted.sort_by(|a, b| js_locale_compare(a, b));
            pairs.push([sorted[0].clone(), sorted[1].clone()]);
        }
    }
    pairs.sort_by(|a, b| js_locale_compare(&a[0], &b[0]));
    pairs
}

// ---------------------------------------------------------------------------
// reciprocalCrossingPairs (private)
// ---------------------------------------------------------------------------

fn reciprocal_crossing_pairs(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
) -> Vec<[String; 2]> {
    let routes: Vec<(&String, &RouteData)> = route_by_id.iter().collect();
    let mut crossing: HashSet<String> = HashSet::new();
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            if route_intersections(routes[i].1, routes[j].1) > 0 {
                crossing.insert(routes[i].0.clone());
                crossing.insert(routes[j].0.clone());
            }
        }
    }
    reciprocal_pairs(route_by_id, rel_by_id)
        .into_iter()
        .filter(|[a, b]| crossing.contains(a) || crossing.contains(b))
        .collect()
}

// ---------------------------------------------------------------------------
// relieveCrowdedSurfaces
// ---------------------------------------------------------------------------

/// Port of JS `relieveCrowdedSurfaces(routeById, relationshipById, input, buildRouteForSides)`.
///
/// Two-phase surgical relief: Phase 1 moves reciprocal crossing pairs jointly onto
/// a shared escape gutter; Phase 2 spills marginal endpoints off over-capacity surfaces.
/// Both phases are gated by the whole-diagram cost guard.
pub struct ReliefResult {
    /// Reciprocal pairs Phase 1 relocated onto a shared gutter.
    pub pairs: Vec<[String; 2]>,
    /// Whether relief changed any route at all.
    pub any_moved: bool,
}

pub fn relieve_crowded_surfaces(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    builder: Option<&dyn BuildRouteForSides>,
) -> ReliefResult {
    let builder = match builder {
        Some(b) => b,
        None => return ReliefResult { pairs: vec![], any_moved: false },
    };
    let rects = extract_rects(input.node_rects);
    let ri = make_route_input(input.visible_node_ids, &rects);

    let surface_over_capacity = |route_by_id: &IndexMap<String, RouteData>,
                                  node_id: &str,
                                  side: &str|
     -> bool {
        let mr = match input.node_rects.get(node_id) {
            Some(r) => r,
            None => return false,
        };
        let surfs = surfaces_of(route_by_id, rel_by_id, input.node_rects);
        let key = format!("{} {}", node_id, side);
        surfs
            .get(&key)
            .map(|s| s.positions.len() > surface_capacity(&mr.rect, side) as usize)
            .unwrap_or(false)
    };

    // frozen_for_endpoint: the endpoint is on its ideal facing side (always frozen),
    // OR faces its partner while within capacity.
    let frozen_for_endpoint =
        |route_by_id: &IndexMap<String, RouteData>,
         rect: &Rect,
         partner_rect: &Rect,
         side: &str,
         node_id: &str|
         -> bool {
            if side == ideal_facing_side(rect, partner_rect) {
                return true;
            }
            side_faces_partner(side, rect, partner_rect)
                && !surface_over_capacity(route_by_id, node_id, side)
        };

    let mut moved_pairs: Vec<[String; 2]> = Vec::new();

    // Phase 1: joint reciprocal-pair moves onto a shared escape gutter.
    let crossing_pairs = reciprocal_crossing_pairs(route_by_id, rel_by_id);
    for [id_a, id_b] in &crossing_pairs {
        let rel_a = match rel_by_id.get(id_a) {
            Some(r) => r,
            None => continue,
        };
        let rel_b = match rel_by_id.get(id_b) {
            Some(r) => r,
            None => continue,
        };
        let route_a = match route_by_id.get(id_a) {
            Some(r) => r.clone(),
            None => continue,
        };
        if route_a.points.is_empty() {
            continue;
        }
        let from_mr = match input.node_rects.get(&rel_a.from) {
            Some(r) => r,
            None => continue,
        };
        let to_mr = match input.node_rects.get(&rel_a.to) {
            Some(r) => r,
            None => continue,
        };
        let start_side = endpoint_side(&from_mr.rect, &route_a.points[0]).to_string();
        let end_side = endpoint_side(&to_mr.rect, route_a.points.last().unwrap()).to_string();
        let start_frozen =
            frozen_for_endpoint(route_by_id, &from_mr.rect, &to_mr.rect, &start_side, &rel_a.from);
        let end_frozen =
            frozen_for_endpoint(route_by_id, &to_mr.rect, &from_mr.rect, &end_side, &rel_a.to);

        for side in SIDES {
            if start_frozen && side != start_side.as_str() {
                continue;
            }
            if end_frozen && side != end_side.as_str() {
                continue;
            }
            if side == start_side.as_str() && side == end_side.as_str() {
                continue;
            }
            let before = mount_assignment_cost(route_by_id, rel_by_id, input);
            let saved = snapshot_routes(route_by_id);
            let new_a = builder.build(rel_a, side, side, route_by_id);
            let new_a = match new_a {
                Some(r) => r,
                None => {
                    restore_routes(route_by_id, &saved);
                    continue;
                }
            };
            let edge_rel_a = make_relationship(rel_a);
            if route_collides_with_non_endpoints(&new_a, &edge_rel_a, &ri) {
                restore_routes(route_by_id, &saved);
                continue;
            }
            route_by_id.insert(id_a.clone(), new_a);
            let new_b = builder.build(rel_b, side, side, route_by_id);
            let new_b = match new_b {
                Some(r) => r,
                None => {
                    restore_routes(route_by_id, &saved);
                    continue;
                }
            };
            let edge_rel_b = make_relationship(rel_b);
            if route_collides_with_non_endpoints(&new_b, &edge_rel_b, &ri) {
                restore_routes(route_by_id, &saved);
                continue;
            }
            route_by_id.insert(id_b.clone(), new_b);
            if mount_assignment_cost(route_by_id, rel_by_id, input) < before {
                moved_pairs.push([id_a.clone(), id_b.clone()]);
                break;
            }
            restore_routes(route_by_id, &saved);
        }
    }

    // Phase 2: spill the marginal endpoint of any surface still over capacity.
    let mut spilled = false;
    let candidate_ids = relief_candidate_ids(route_by_id, rel_by_id, input.node_rects);
    'outer: for id in &candidate_ids {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        let route = match route_by_id.get(id) {
            Some(r) => r.clone(),
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        let from_mr = match input.node_rects.get(&rel.from) {
            Some(r) => r,
            None => continue,
        };
        let to_mr = match input.node_rects.get(&rel.to) {
            Some(r) => r,
            None => continue,
        };
        let start_side = endpoint_side(&from_mr.rect, &route.points[0]).to_string();
        let end_side = endpoint_side(&to_mr.rect, route.points.last().unwrap()).to_string();
        if !surface_over_capacity(route_by_id, &rel.from, &start_side)
            && !surface_over_capacity(route_by_id, &rel.to, &end_side)
        {
            continue;
        }
        let start_frozen =
            frozen_for_endpoint(route_by_id, &from_mr.rect, &to_mr.rect, &start_side, &rel.from);
        let end_frozen =
            frozen_for_endpoint(route_by_id, &to_mr.rect, &from_mr.rect, &end_side, &rel.to);

        for candidate_start in SIDES {
            if start_frozen && candidate_start != start_side.as_str() {
                continue;
            }
            for candidate_end in SIDES {
                if end_frozen && candidate_end != end_side.as_str() {
                    continue;
                }
                if candidate_start == start_side.as_str() && candidate_end == end_side.as_str() {
                    continue;
                }
                let before = mount_assignment_cost(route_by_id, rel_by_id, input);
                let saved = snapshot_routes(route_by_id);
                let rebuilt = builder.build(rel, candidate_start, candidate_end, route_by_id);
                let rebuilt = match rebuilt {
                    Some(r) => r,
                    None => continue,
                };
                let edge_rel = make_relationship(rel);
                if route_collides_with_non_endpoints(&rebuilt, &edge_rel, &ri) {
                    continue;
                }
                route_by_id.insert(id.clone(), rebuilt);
                if mount_assignment_cost(route_by_id, rel_by_id, input) < before {
                    spilled = true;
                    continue 'outer;
                }
                restore_routes(route_by_id, &saved);
            }
        }
    }

    ReliefResult {
        pairs: moved_pairs.clone(),
        any_moved: !moved_pairs.is_empty() || spilled,
    }
}

// ---------------------------------------------------------------------------
// buildReciprocalGutterBridge
// ---------------------------------------------------------------------------

/// Port of JS `buildReciprocalGutterBridge(...)`.
pub struct GutterBridge {
    pub request: RouteData,
    pub ret: RouteData,
}

pub fn build_reciprocal_gutter_bridge(
    request_rel: &MountRelationship,
    _return_rel: &MountRelationship,
    request_route: &RouteData,
    return_route: &RouteData,
    node_rects: &IndexMap<String, MountRect>,
    side: &str,
    gutter_clearance: f64,
) -> Option<GutterBridge> {
    let ra = node_rects.get(&request_rel.from).map(|mr| &mr.rect)?;
    let rb = node_rects.get(&request_rel.to).map(|mr| &mr.rect)?;
    const PAD: f64 = 8.0;
    let surf_ya = if side == "top" { ra.y } else { ra.y + ra.height };
    let surf_yb = if side == "top" { rb.y } else { rb.y + rb.height };
    let a_cx = ra.x + ra.width / 2.0;
    let b_cx = rb.x + rb.width / 2.0;
    // JS Math.sign, 0 → fallback 1
    let toward_b = {
        let s = (b_cx - a_cx).signum();
        if s == 0.0 { 1.0 } else { s }
    };
    let clamp_x = |rect: &Rect, x: f64| -> f64 {
        f64::max(rect.x + PAD, f64::min(rect.x + rect.width - PAD, x))
    };
    let req_ax = clamp_x(ra, a_cx + toward_b * BRIDGE_MOUNT_OFFSET);
    let ret_ax = clamp_x(ra, a_cx - toward_b * BRIDGE_MOUNT_OFFSET);
    let req_bx = clamp_x(rb, b_cx - toward_b * BRIDGE_MOUNT_OFFSET);
    let ret_bx = clamp_x(rb, b_cx + toward_b * BRIDGE_MOUNT_OFFSET);
    let edge = if side == "top" {
        f64::min(ra.y, rb.y) - gutter_clearance
    } else {
        f64::max(ra.y + ra.height, rb.y + rb.height) + gutter_clearance
    };
    let lane_req = edge;
    let lane_ret = if side == "top" {
        edge - BRIDGE_LANE_GAP
    } else {
        edge + BRIDGE_LANE_GAP
    };
    let request = route_with_points(
        request_route,
        vec![
            Point { x: req_ax, y: surf_ya },
            Point { x: req_ax, y: lane_req },
            Point { x: req_bx, y: lane_req },
            Point { x: req_bx, y: surf_yb },
        ],
        request_route.controls.clone(),
    );
    let ret = route_with_points(
        return_route,
        vec![
            Point { x: ret_bx, y: surf_yb },
            Point { x: ret_bx, y: lane_ret },
            Point { x: ret_ax, y: lane_ret },
            Point { x: ret_ax, y: surf_ya },
        ],
        return_route.controls.clone(),
    );
    Some(GutterBridge { request, ret })
}

// ---------------------------------------------------------------------------
// buildMonotonicStaircase
// ---------------------------------------------------------------------------

/// Port of JS `buildMonotonicStaircase(requestRoute, startSide, endSide, elbow)`.
pub fn build_monotonic_staircase(
    request_route: &RouteData,
    start_side: &str,
    end_side: &str,
    elbow: f64,
) -> RouteData {
    let p_a = &request_route.points[0];
    let p_b = request_route.points.last().unwrap();
    let horiz = |side: &str| side == "left" || side == "right";
    let points = if horiz(start_side) && horiz(end_side) {
        if p_a.y == p_b.y {
            vec![p_a.clone(), p_b.clone()]
        } else {
            vec![
                p_a.clone(),
                Point { x: elbow, y: p_a.y },
                Point { x: elbow, y: p_b.y },
                p_b.clone(),
            ]
        }
    } else if !horiz(start_side) && !horiz(end_side) {
        if p_a.x == p_b.x {
            vec![p_a.clone(), p_b.clone()]
        } else {
            vec![
                p_a.clone(),
                Point { x: p_a.x, y: elbow },
                Point { x: p_b.x, y: elbow },
                p_b.clone(),
            ]
        }
    } else {
        // Mixed L
        let corner = if horiz(start_side) {
            Point { x: p_b.x, y: p_a.y }
        } else {
            Point { x: p_a.x, y: p_b.y }
        };
        vec![p_a.clone(), corner, p_b.clone()]
    };
    route_with_points(request_route, points, request_route.controls.clone())
}

// ---------------------------------------------------------------------------
// clearElbows (private)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn clear_elbows(
    node_rects: &IndexMap<String, MountRect>,
    visible_node_ids: &[String],
    axis: &str,
    lo: f64,
    hi: f64,
    band_lo: f64,
    band_hi: f64,
    max: usize,
) -> Vec<f64> {
    let a = f64::min(lo, hi);
    let b = f64::max(lo, hi);
    let mut occupied: Vec<[f64; 2]> = Vec::new();
    for id in visible_node_ids {
        let r = match node_rects.get(id) {
            Some(mr) => &mr.rect,
            None => continue,
        };
        let (span_lo, span_hi) = if axis == "x" {
            (r.y, r.y + r.height)
        } else {
            (r.x, r.x + r.width)
        };
        if span_hi <= band_lo || span_lo >= band_hi {
            continue;
        }
        let (s, e) = if axis == "x" {
            (r.x, r.x + r.width)
        } else {
            (r.y, r.y + r.height)
        };
        occupied.push([s, e]);
    }
    occupied.sort_by(|p, q| p[0].partial_cmp(&q[0]).unwrap_or(std::cmp::Ordering::Equal));
    let mut gutters: Vec<f64> = Vec::new();
    let mut cursor = a;
    for [s, e] in &occupied {
        if *s > cursor {
            gutters.push((cursor + f64::min(*s, b)) / 2.0);
        }
        cursor = f64::max(cursor, *e);
        if cursor >= b {
            break;
        }
    }
    if cursor < b {
        gutters.push((cursor + b) / 2.0);
    }
    let mid = (a + b) / 2.0;
    gutters.retain(|&g| g > a && g < b);
    gutters.sort_by(|p, q| {
        (p - mid)
            .abs()
            .partial_cmp(&(q - mid).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    gutters.truncate(max);
    gutters
}

// ---------------------------------------------------------------------------
// routeNonFacingCount (private)
// ---------------------------------------------------------------------------

fn route_non_facing_count(
    route: &RouteData,
    rel: &MountRelationship,
    node_rects: &IndexMap<String, MountRect>,
    lane_index_by_node: &IndexMap<String, i64>,
    row_index_by_node: &IndexMap<String, i64>,
) -> usize {
    let from_mr = match node_rects.get(&rel.from) {
        Some(r) => r,
        None => return 0,
    };
    let to_mr = match node_rects.get(&rel.to) {
        Some(r) => r,
        None => return 0,
    };
    let intent_rel = make_intent_relationship(rel);
    let intent = derive_route_intent(&DeriveRouteIntentInput {
        relationship: &intent_rel,
        from_rect: &from_mr.rect,
        to_rect: &to_mr.rect,
        from_lane_index: *lane_index_by_node.get(&rel.from).unwrap_or(&0),
        to_lane_index: *lane_index_by_node.get(&rel.to).unwrap_or(&0),
        from_row_index: *row_index_by_node.get(&rel.from).unwrap_or(&0),
        to_row_index: *row_index_by_node.get(&rel.to).unwrap_or(&0),
    });
    let mut count = 0usize;
    if endpoint_side(&from_mr.rect, &route.points[0]) != intent.expected_source_side {
        count += 1;
    }
    if endpoint_side(&to_mr.rect, route.points.last().unwrap()) != intent.expected_target_side {
        count += 1;
    }
    count
}

// ---------------------------------------------------------------------------
// routeUnjustifiedNonFacing
// ---------------------------------------------------------------------------

/// Port of JS `routeUnjustifiedNonFacing(route, rel, input)`.
pub fn route_unjustified_non_facing(
    route: &RouteData,
    rel: &MountRelationship,
    input: &MountInput<'_>,
) -> usize {
    let from_mr = match input.node_rects.get(&rel.from) {
        Some(r) => r,
        None => return 0,
    };
    let to_mr = match input.node_rects.get(&rel.to) {
        Some(r) => r,
        None => return 0,
    };
    let intent_rel = make_intent_relationship(rel);
    let intent = derive_route_intent(&DeriveRouteIntentInput {
        relationship: &intent_rel,
        from_rect: &from_mr.rect,
        to_rect: &to_mr.rect,
        from_lane_index: *input.lane_index_by_node.get(&rel.from).unwrap_or(&0),
        to_lane_index: *input.lane_index_by_node.get(&rel.to).unwrap_or(&0),
        from_row_index: *input.row_index_by_node.get(&rel.from).unwrap_or(&0),
        to_row_index: *input.row_index_by_node.get(&rel.to).unwrap_or(&0),
    });
    let source_side = endpoint_side(&from_mr.rect, &route.points[0]);
    let target_side = endpoint_side(&to_mr.rect, route.points.last().unwrap());
    if source_side == intent.expected_source_side && target_side == intent.expected_target_side {
        return 0;
    }
    let blocker_rects: Vec<Rect> = input
        .visible_node_ids
        .iter()
        .filter(|nid| *nid != &rel.from && *nid != &rel.to)
        .filter_map(|nid| input.node_rects.get(nid).map(|mr| mr.rect.clone()))
        .collect();
    let options = semantic_surface_options(&SemanticSurfaceOptionsInput {
        expected_sides: SidePair {
            source: intent.expected_source_side.clone(),
            target: intent.expected_target_side.clone(),
        },
        relationship: &intent_rel,
        from_rect: &from_mr.rect,
        to_rect: &to_mr.rect,
        blocker_rects,
        canvas_width: input.canvas_width,
        canvas_height: input.canvas_height,
    });
    let mut count = 0usize;
    if source_side != intent.expected_source_side && !options.source.contains(source_side) {
        count += 1;
    }
    if target_side != intent.expected_target_side && !options.target.contains(target_side) {
        count += 1;
    }
    count
}

// ---------------------------------------------------------------------------
// totalNonFacing (private)
// ---------------------------------------------------------------------------

fn total_non_facing(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) -> usize {
    let mut total = 0usize;
    for (id, route) in route_by_id {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        if rel.relationship_type != "flow" || route.points.is_empty() {
            continue;
        }
        total += route_unjustified_non_facing(route, rel, input);
    }
    total
}

// ---------------------------------------------------------------------------
// noHardFactorWorsening / facingPolishCost (private)
// ---------------------------------------------------------------------------

fn no_hard_factor_worsening(before: &MountCostFactors, after: &MountCostFactors) -> bool {
    // bend, length, intentMismatch are excluded (polish — allowed to rise).
    macro_rules! check {
        ($field:ident) => {
            if after.$field > before.$field {
                return false;
            }
        };
    }
    check!(collision);
    check!(endpoint_traversal);
    check!(repeated_crossing);
    check!(self_overlap);
    check!(shared_segment);
    check!(shared_segment_length);
    check!(perimeter_fallback);
    check!(crossing);
    check!(monotonic_backtrack);
    check!(dogleg);
    check!(shallow_jog);
    check!(cramped);
    check!(over_capacity);
    true
}

fn facing_polish_cost(non_facing: usize, factors: &MountCostFactors) -> f64 {
    (non_facing as f64) * MOUNT_COST.intent_mismatch
        + factors.bend * MOUNT_COST.bend
        + factors.length * MOUNT_COST.length
}

// ---------------------------------------------------------------------------
// tryIntentFacingMoves (private)
// ---------------------------------------------------------------------------

fn try_intent_facing_moves(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    builder: Option<&dyn BuildRouteForSides>,
) {
    let builder = match builder {
        Some(b) => b,
        None => return,
    };
    let rects = extract_rects(input.node_rects);
    let ri = make_route_input(input.visible_node_ids, &rects);

    let ids: Vec<String> = {
        let mut v: Vec<String> = route_by_id.keys().cloned().collect();
        v.sort_by(|a, b| js_locale_compare(a, b));
        v
    };

    for id in &ids {
        let rel = match rel_by_id.get(id) {
            Some(r) => r,
            None => continue,
        };
        let route = match route_by_id.get(id) {
            Some(r) => r.clone(),
            None => continue,
        };
        if route.points.is_empty() || rel.relationship_type != "flow" {
            continue;
        }
        if rel.preferred_start_side.is_some() || rel.preferred_end_side.is_some() {
            continue;
        }
        let from_mr = match input.node_rects.get(&rel.from) {
            Some(r) => r,
            None => continue,
        };
        let to_mr = match input.node_rects.get(&rel.to) {
            Some(r) => r,
            None => continue,
        };
        if from_mr.fixed_ports || to_mr.fixed_ports {
            continue;
        }
        if route_unjustified_non_facing(&route, rel, input) == 0 {
            continue;
        }
        let start_side = endpoint_side(&from_mr.rect, &route.points[0]).to_string();
        let end_side = endpoint_side(&to_mr.rect, route.points.last().unwrap()).to_string();
        let before_factors = mount_cost_factors(route_by_id, rel_by_id, input);
        let before_polish =
            facing_polish_cost(total_non_facing(route_by_id, rel_by_id, input), &before_factors);
        let saved = snapshot_routes(route_by_id);
        let mut best_polish = before_polish;
        let mut best_state: Option<IndexMap<String, RouteData>> = None;

        for cand_start in SIDES {
            for cand_end in SIDES {
                if cand_start == start_side.as_str() && cand_end == end_side.as_str() {
                    continue;
                }
                let rebuilt = builder.build(rel, cand_start, cand_end, route_by_id);
                let rebuilt = match rebuilt {
                    Some(r) => r,
                    None => {
                        restore_routes(route_by_id, &saved);
                        continue;
                    }
                };
                if rebuilt.points.is_empty() {
                    restore_routes(route_by_id, &saved);
                    continue;
                }
                let edge_rel = make_relationship(rel);
                if route_collides_with_non_endpoints(&rebuilt, &edge_rel, &ri) {
                    restore_routes(route_by_id, &saved);
                    continue;
                }
                route_by_id.insert(id.clone(), rebuilt);
                respread_surfaces(route_by_id, rel_by_id, input.node_rects);
                let factors = mount_cost_factors(route_by_id, rel_by_id, input);
                let polish = facing_polish_cost(total_non_facing(route_by_id, rel_by_id, input), &factors);
                if polish < best_polish && no_hard_factor_worsening(&before_factors, &factors) {
                    best_polish = polish;
                    best_state = Some(snapshot_routes(route_by_id));
                }
                restore_routes(route_by_id, &saved);
            }
        }
        if let Some(state) = best_state {
            restore_routes(route_by_id, &state);
        }
    }
}

// ---------------------------------------------------------------------------
// reciprocalParallelMoves
// ---------------------------------------------------------------------------

/// Port of JS `reciprocalParallelMoves(routeById, relationshipById, input, buildRouteForSides)`.
pub fn reciprocal_parallel_moves(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    builder: Option<&dyn BuildRouteForSides>,
) {
    let rects = extract_rects(input.node_rects);
    let ri = make_route_input(input.visible_node_ids, &rects);

    // Group relationships by sorted node-pair key (flow edges only).
    let mut by_node_pair: IndexMap<String, Vec<MountRelationship>> = IndexMap::new();
    for rel in rel_by_id.values() {
        if rel.relationship_type != "flow" || !route_by_id.contains_key(&rel.id) {
            continue;
        }
        let mut nodes = [rel.from.clone(), rel.to.clone()];
        nodes.sort();
        let key = format!("{} {}", nodes[0], nodes[1]);
        by_node_pair.entry(key).or_default().push(rel.clone());
    }

    for group in by_node_pair.values() {
        if group.len() < 2 {
            continue;
        }
        // Sort by displayIndex for stable pairing.
        let mut sorted = group.clone();
        sorted.sort_by_key(|a| a.display_index);

        let mut paired: HashSet<String> = HashSet::new();
        // Collect (request, ret) pairs to process.
        let mut work_pairs: Vec<(MountRelationship, MountRelationship)> = Vec::new();
        for request in &sorted {
            if paired.contains(&request.id) {
                continue;
            }
            // Find nearest un-paired return (opposite direction, displayIndex >=)
            let ret = sorted.iter().find(|o| {
                !paired.contains(&o.id)
                    && o.id != request.id
                    && o.from == request.to
                    && o.to == request.from
                    && o.display_index >= request.display_index
            });
            let ret = match ret {
                Some(r) => r,
                None => continue,
            };
            paired.insert(request.id.clone());
            paired.insert(ret.id.clone());
            work_pairs.push((request.clone(), ret.clone()));
        }

        for (request, ret) in &work_pairs {
            let saved_request = match route_by_id.get(&request.id) {
                Some(r) => r.clone(),
                None => continue,
            };
            let saved_return = match route_by_id.get(&ret.id) {
                Some(r) => r.clone(),
                None => continue,
            };
            if saved_request.points.is_empty() || saved_return.points.is_empty() {
                continue;
            }

            let mut coupled: Vec<(RouteData, RouteData)> = Vec::new();

            // Plain mirror: reversed request ± offset.
            let reversed: Vec<Point> = saved_request.points.iter().rev().cloned().collect();
            for delta in [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET] {
                let offset_pts = offset_orthogonal_polyline(&reversed, delta);
                coupled.push((
                    saved_request.clone(),
                    route_with_points(&saved_return, offset_pts, saved_return.controls.clone()),
                ));
            }

            // Staircase candidates.
            let ras = input.node_rects.get(&request.from).map(|mr| &mr.rect);
            let rbs = input.node_rects.get(&request.to).map(|mr| &mr.rect);
            if let (Some(ras), Some(rbs)) = (ras, rbs) {
                let p_a = &saved_request.points[0];
                let p_b = saved_request.points.last().unwrap();
                let start_side = endpoint_side(ras, p_a);
                let end_side = endpoint_side(rbs, p_b);
                let horiz = |s: &str| s == "left" || s == "right";
                let elbows = if horiz(start_side) && horiz(end_side) {
                    clear_elbows(
                        input.node_rects,
                        input.visible_node_ids,
                        "x",
                        p_a.x,
                        p_b.x,
                        f64::min(p_a.y, p_b.y),
                        f64::max(p_a.y, p_b.y),
                        4,
                    )
                } else if !horiz(start_side) && !horiz(end_side) {
                    clear_elbows(
                        input.node_rects,
                        input.visible_node_ids,
                        "y",
                        p_a.y,
                        p_b.y,
                        f64::min(p_a.x, p_b.x),
                        f64::max(p_a.x, p_b.x),
                        4,
                    )
                } else {
                    vec![0.0]
                };
                for elbow in &elbows {
                    let staircase = build_monotonic_staircase(&saved_request, start_side, end_side, *elbow);
                    let reversed_stair: Vec<Point> = staircase.points.iter().rev().cloned().collect();
                    for delta in [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET] {
                        let offset_pts = offset_orthogonal_polyline(&reversed_stair, delta);
                        coupled.push((
                            staircase.clone(),
                            route_with_points(&saved_return, offset_pts, saved_return.controls.clone()),
                        ));
                    }
                }
            }

            // Gutter bridge candidates.
            let ra = input.node_rects.get(&request.from).map(|mr| &mr.rect);
            let rb = input.node_rects.get(&request.to).map(|mr| &mr.rect);
            let lane_step = MIN_LEGIBLE_GAP * 2.0;
            if let (Some(ra), Some(rb)) = (ra, rb) {
                for side in ["top", "bottom"] {
                    let headroom = if side == "top" {
                        f64::min(ra.y, rb.y) - MIN_LEGIBLE_GAP
                    } else {
                        let canvas_h = if input.canvas_height == 0.0 {
                            f64::INFINITY
                        } else {
                            input.canvas_height
                        };
                        canvas_h - f64::max(ra.y + ra.height, rb.y + rb.height) - MIN_LEGIBLE_GAP
                    };
                    for lane in 0..BRIDGE_MAX_LANES {
                        let clearance = BRIDGE_GUTTER_CLEARANCE + lane as f64 * lane_step + BRIDGE_LANE_GAP;
                        if clearance > headroom {
                            break;
                        }
                        if let Some(bridge) = build_reciprocal_gutter_bridge(
                            request,
                            ret,
                            &saved_request,
                            &saved_return,
                            input.node_rects,
                            side,
                            clearance,
                        ) {
                            coupled.push((bridge.request, bridge.ret));
                        }
                    }
                }
            }

            // Coupled perpendicular-escape candidates (when builder available).
            if let Some(b) = builder {
                if let (Some(ra), Some(_rb)) = (
                    input.node_rects.get(&request.from),
                    input.node_rects.get(&request.to),
                ) {
                    let req_start = endpoint_side(&ra.rect, &saved_request.points[0]);
                    let req_end = {
                        let to_mr = input.node_rects.get(&request.to).unwrap();
                        endpoint_side(&to_mr.rect, saved_request.points.last().unwrap())
                    };
                    for cand_start in SIDES {
                        for cand_end in SIDES {
                            if cand_start == req_start && cand_end == req_end {
                                continue;
                            }
                            let rebuilt_request = b.build(request, cand_start, cand_end, route_by_id);
                            let rebuilt_request = match rebuilt_request {
                                Some(r) if !r.points.is_empty() => r,
                                _ => continue,
                            };
                            let reversed_rebuilt: Vec<Point> =
                                rebuilt_request.points.iter().rev().cloned().collect();
                            for delta in [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET] {
                                let offset_pts =
                                    offset_orthogonal_polyline(&reversed_rebuilt, delta);
                                coupled.push((
                                    rebuilt_request.clone(),
                                    route_with_points(
                                        &saved_return,
                                        offset_pts,
                                        saved_return.controls.clone(),
                                    ),
                                ));
                            }
                        }
                    }
                }
            }

            // Evaluate candidates.
            let before_factors = mount_cost_factors(route_by_id, rel_by_id, input);
            let before_cost = weighted_mount_cost(&before_factors);
            let saved_non_facing = route_non_facing_count(
                &saved_request,
                request,
                input.node_rects,
                input.lane_index_by_node,
                input.row_index_by_node,
            ) + route_non_facing_count(
                &saved_return,
                ret,
                input.node_rects,
                input.lane_index_by_node,
                input.row_index_by_node,
            );

            let mut best_cost = before_cost;
            let mut best_request = saved_request.clone();
            let mut best_return = saved_return.clone();

            let req_edge_rel = make_relationship(request);
            let ret_edge_rel = make_relationship(ret);

            for (cand_req, cand_ret) in &coupled {
                if route_collides_with_non_endpoints(cand_req, &req_edge_rel, &ri) {
                    continue;
                }
                if route_collides_with_non_endpoints(cand_ret, &ret_edge_rel, &ri) {
                    continue;
                }
                // Temporarily set candidates to measure cost.
                route_by_id.insert(request.id.clone(), cand_req.clone());
                route_by_id.insert(ret.id.clone(), cand_ret.clone());
                let factors = mount_cost_factors(route_by_id, rel_by_id, input);
                route_by_id.insert(request.id.clone(), saved_request.clone());
                route_by_id.insert(ret.id.clone(), saved_return.clone());
                let cost = weighted_mount_cost(&factors);
                if cost >= best_cost {
                    continue;
                }
                // Never add a crossing.
                if factors.crossing > before_factors.crossing {
                    continue;
                }
                // Facing guard.
                let candidate_non_facing = route_non_facing_count(
                    cand_req,
                    request,
                    input.node_rects,
                    input.lane_index_by_node,
                    input.row_index_by_node,
                ) + route_non_facing_count(
                    cand_ret,
                    ret,
                    input.node_rects,
                    input.lane_index_by_node,
                    input.row_index_by_node,
                );
                if candidate_non_facing > saved_non_facing
                    && factors.crossing >= before_factors.crossing
                {
                    continue;
                }
                best_cost = cost;
                best_request = cand_req.clone();
                best_return = cand_ret.clone();
            }
            route_by_id.insert(request.id.clone(), best_request);
            route_by_id.insert(ret.id.clone(), best_return);
        }
    }
}

// ---------------------------------------------------------------------------
// optimizeMountAssignments
// ---------------------------------------------------------------------------

/// Port of JS `optimizeMountAssignments(routeById, relationshipById, input, options)`.
pub fn optimize_mount_assignments(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    builder: Option<&dyn BuildRouteForSides>,
) {
    for _iter in 0..MOUNT_MAX_ITERS {
        let before = mount_assignment_cost(route_by_id, rel_by_id, input);
        let saved = snapshot_routes(route_by_id);
        respread_surfaces(route_by_id, rel_by_id, input.node_rects);
        try_side_moves(route_by_id, rel_by_id, input, builder);
        if mount_assignment_cost(route_by_id, rel_by_id, input) >= before {
            restore_routes(route_by_id, &saved);
            break;
        }
    }
    reciprocal_parallel_moves(route_by_id, rel_by_id, input, builder);
    try_intent_facing_moves(route_by_id, rel_by_id, input, builder);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_edges::RouteData;
    use crate::model::{Point, Rect};
    use indexmap::IndexMap;

    /// Build a minimal orthogonal route from a point list.
    fn mk_route(points: Vec<Point>) -> RouteData {
        use crate::route_geometry::bounds_for_points;
        let all: Vec<Point> = points.clone();
        let sb = bounds_for_points(&all);
        RouteData {
            d: String::new(),
            points,
            controls: None,
            samples: vec![],
            sample_bounds: sb,
            bends: 0,
            label_x: 0.0,
            label_y: 0.0,
            style: "orthogonal".to_string(),
            extra: indexmap::IndexMap::new(),
        }
    }

    fn mk_rect(x: f64, y: f64, w: f64, h: f64) -> MountRect {
        MountRect { rect: Rect { x, y, width: w, height: h }, fixed_ports: false }
    }

    fn mk_rel(id: &str, from: &str, to: &str) -> MountRelationship {
        MountRelationship {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            relationship_type: "flow".to_string(),
            preferred_start_side: None,
            preferred_end_side: None,
            display_index: 0,
            kind: None,
            return_of: None,
            outcome: None,
            step_id: None,
            flow_id: None,
        }
    }

    // -----------------------------------------------------------------------
    // surfaceCrampedUnits — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn cramped_units_positions_within_gap() {
        // [10, 14] in length 40: guards=[0,10,14,40], gaps=[10,4,26]
        // gap=4 == MIN_LEGIBLE_GAP (4.0), NOT < 4 → no shortfall
        // Node: surfaceCrampedUnits([10, 14], 40) = 0
        assert_eq!(surface_cramped_units(&[10.0, 14.0], 40.0), 0.0);
    }

    #[test]
    fn cramped_units_empty_positions() {
        // Node: surfaceCrampedUnits([], 40) = 0
        assert_eq!(surface_cramped_units(&[], 40.0), 0.0);
    }

    #[test]
    fn cramped_units_single_position() {
        // Node: surfaceCrampedUnits([10], 40) = 0
        assert_eq!(surface_cramped_units(&[10.0], 40.0), 0.0);
    }

    #[test]
    fn cramped_units_three_tight_positions() {
        // [5, 7, 10] in length 20: guards=[0,5,7,10,20]
        // gaps = [5, 2, 3, 10] — gap 2 < 4 (shortfall 2), gap 3 < 4 (shortfall 1)
        // Node: surfaceCrampedUnits([5, 7, 10], 20) = 3
        assert_eq!(surface_cramped_units(&[5.0, 7.0, 10.0], 20.0), 3.0);
    }

    // -----------------------------------------------------------------------
    // excessLength — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn excess_length_straight_edge() {
        // Route [{x:0,y:0},{x:100,y:0}], from rect 0..10 to rect 90..100
        // nodeGapLength = 90 - 10 = 80; routeLength = 100; excess = 20
        // Node: excessLength = 20
        let route = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let from_rect = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 0.0, width: 10.0, height: 10.0 };
        assert_eq!(excess_length(&route, Some(&from_rect), Some(&to_rect)), 20.0);
    }

    #[test]
    fn excess_length_detour() {
        // L-shaped route from (0,0)→(50,0)→(50,50)→(100,50): length=150
        // fromRect 0,0,10,10 toRect 90,40,10,10: gapX=80, gapY=30, gap=110
        // Node: excessLength = 40
        let route = mk_route(vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 50.0, y: 0.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 100.0, y: 50.0 },
        ]);
        let from_rect = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 40.0, width: 10.0, height: 10.0 };
        assert_eq!(excess_length(&route, Some(&from_rect), Some(&to_rect)), 40.0);
    }

    // -----------------------------------------------------------------------
    // doglegCount — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn dogleg_count_straight_no_dogleg() {
        // Node: doglegCount(straight, A, B) = 0
        let route = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let from_rect = Rect { x: 0.0, y: 20.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 20.0, width: 10.0, height: 10.0 };
        assert_eq!(dogleg_count(&route, Some(&from_rect), Some(&to_rect)), 0.0);
    }

    #[test]
    fn dogleg_count_backtrack() {
        // [{x:0,y:0},{x:60,y:0},{x:40,y:0},{x:100,y:0}]: goes right then back left then right
        // x_dir=1 (to right of from). The segment 60→40 has dx<0 = -x_dir → count 1
        // Node: doglegCount = 1
        let route = mk_route(vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 60.0, y: 0.0 },
            Point { x: 40.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
        ]);
        let from_rect = Rect { x: 0.0, y: 20.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 20.0, width: 10.0, height: 10.0 };
        assert_eq!(dogleg_count(&route, Some(&from_rect), Some(&to_rect)), 1.0);
    }

    // -----------------------------------------------------------------------
    // strictCrossingCount / routeIntersections — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn strict_crossing_counts_interior_x() {
        // H: (0,50)-(100,50); V: (50,0)-(50,100) — strictly straddle each other
        // Node: strictCrossingCount = 1
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let v = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 100.0 }]);
        assert_eq!(strict_crossing_count(&h, &v), 1.0);
    }

    #[test]
    fn strict_crossing_misses_t_junction() {
        // H: (0,50)-(100,50); V-T: (50,0)-(50,50) — touches endpoint, not interior
        // Node: strictCrossingCount = 0
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let vt = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 50.0 }]);
        assert_eq!(strict_crossing_count(&h, &vt), 0.0);
    }

    #[test]
    fn route_intersections_counts_t_junction() {
        // Node: routeIntersections for T-junction = 1
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let vt = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 50.0 }]);
        assert_eq!(route_intersections(&h, &vt), 1);
    }

    #[test]
    fn route_intersections_crossing() {
        // Node: routeIntersections for clean X = 1
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let v = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 100.0 }]);
        assert_eq!(route_intersections(&h, &v), 1);
    }

    // -----------------------------------------------------------------------
    // mountCostFactors — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn mount_cost_factors_crossing_diagram() {
        // Two routes that strictly cross; from Node run:
        // factors.crossing = 1, factors.intentMismatch = 4, factors.length = 40
        // cost = 3000*1 + 1500*4 + 6*40 = 3000+6000+240 = 9240
        let mut route_by_id: IndexMap<String, RouteData> = IndexMap::new();
        route_by_id.insert("e1".to_string(), mk_route(vec![
            Point { x: 0.0, y: 50.0 }, Point { x: 200.0, y: 50.0 }
        ]));
        route_by_id.insert("e2".to_string(), mk_route(vec![
            Point { x: 100.0, y: 0.0 }, Point { x: 100.0, y: 100.0 }
        ]));

        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("e1".to_string(), mk_rel("e1", "A", "B"));
        rel_by_id.insert("e2".to_string(), mk_rel("e2", "C", "D"));

        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("A".to_string(), mk_rect(0.0, 45.0, 10.0, 10.0));
        node_rects.insert("B".to_string(), mk_rect(190.0, 45.0, 10.0, 10.0));
        node_rects.insert("C".to_string(), mk_rect(95.0, 0.0, 10.0, 10.0));
        node_rects.insert("D".to_string(), mk_rect(95.0, 90.0, 10.0, 10.0));

        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string(), "D".to_string()];
        let lane_idx: IndexMap<String, i64> = IndexMap::new();
        let row_idx: IndexMap<String, i64> = IndexMap::new();
        let input = MountInput {
            visible_node_ids: &visible,
            node_rects: &node_rects,
            lane_index_by_node: &lane_idx,
            row_index_by_node: &row_idx,
            canvas_width: 400.0,
            canvas_height: 200.0,
        };

        let factors = mount_cost_factors(&route_by_id, &rel_by_id, &input);
        assert_eq!(factors.crossing, 1.0, "crossing");
        assert_eq!(factors.intent_mismatch, 4.0, "intentMismatch");
        assert_eq!(factors.length, 40.0, "length");
        assert_eq!(factors.collision, 0.0, "collision");
        assert_eq!(factors.shared_segment, 0.0, "sharedSegment");

        let cost = mount_assignment_cost(&route_by_id, &rel_by_id, &input);
        assert_eq!(cost, 9240.0);
    }

    // -----------------------------------------------------------------------
    // surfacesOf — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn surfaces_of_single_horizontal_route() {
        // e1: A(left side 0,45,10,10) → B(right side 90,45,10,10)
        // route (0,50)→(100,50): point 0 is on A.left, point-1 on B.right
        // A.left: axisStart = rect.y = 45; pos = point.y - axisStart = 50-45 = 5
        // B.right: axisStart = rect.y = 45; pos = 50-45 = 5
        // Node: surfaces keys = ["A left","B right"], positions both [5]
        let mut route_by_id: IndexMap<String, RouteData> = IndexMap::new();
        route_by_id.insert("e1".to_string(), mk_route(vec![
            Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }
        ]));
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("e1".to_string(), mk_rel("e1", "A", "B"));
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("A".to_string(), mk_rect(0.0, 45.0, 10.0, 10.0));
        node_rects.insert("B".to_string(), mk_rect(90.0, 45.0, 10.0, 10.0));

        let surfs = surfaces_of(&route_by_id, &rel_by_id, &node_rects);
        assert!(surfs.contains_key("A left"), "A left key missing");
        assert!(surfs.contains_key("B right"), "B right key missing");
        assert_eq!(surfs["A left"].positions, vec![5.0]);
        assert_eq!(surfs["B right"].positions, vec![5.0]);
    }

    // -----------------------------------------------------------------------
    // buildMonotonicStaircase — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn monotonic_staircase_left_right_keeps_points_when_same_y() {
        // start=right, end=left, p_a.y == p_b.y → [pA, pB]
        let route = mk_route(vec![
            Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }
        ]);
        let result = build_monotonic_staircase(&route, "right", "left", 50.0);
        assert_eq!(result.points.len(), 2);
        assert_eq!(result.points[0].x, 0.0);
        assert_eq!(result.points[1].x, 100.0);
    }

    #[test]
    fn monotonic_staircase_right_left_different_y() {
        // Node: staircase route from (0,50)→(50,50)→(50,100)→(100,100), startSide=right, endSide=left, elbow=50
        // horiz both → pA.y(50) != pB.y(100) → [pA, {x:elbow,y:pA.y}, {x:elbow,y:pB.y}, pB]
        // = [{x:0,y:50},{x:50,y:50},{x:50,y:100},{x:100,y:100}]
        // Node confirms staircase points = same as input since they already form a staircase
        let route = mk_route(vec![
            Point { x: 0.0, y: 50.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 50.0, y: 100.0 },
            Point { x: 100.0, y: 100.0 },
        ]);
        let result = build_monotonic_staircase(&route, "right", "left", 50.0);
        // pA = first point = (0,50), pB = last = (100,100)
        // horiz(right) && horiz(left), pA.y=50 != pB.y=100
        // points = [(0,50),(50,50),(50,100),(100,100)]
        assert_eq!(result.points[0], Point { x: 0.0, y: 50.0 });
        assert_eq!(result.points[1], Point { x: 50.0, y: 50.0 });
        assert_eq!(result.points[2], Point { x: 50.0, y: 100.0 });
        assert_eq!(result.points[3], Point { x: 100.0, y: 100.0 });
    }

    // -----------------------------------------------------------------------
    // buildReciprocalGutterBridge — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn reciprocal_gutter_bridge_top() {
        // Node: bridge.request.points = [{x:12,y:40},{x:12,y:26},{x:88,y:26},{x:88,y:40}]
        //       bridge.return.points  = [{x:92,y:40},{x:92,y:12},{x:8,y:12},{x:8,y:40}]
        let req_rel = mk_rel("e1", "A", "B");
        let ret_rel = mk_rel("e2", "B", "A");
        let req_route = mk_route(vec![Point { x: 5.0, y: 50.0 }, Point { x: 95.0, y: 50.0 }]);
        let ret_route = mk_route(vec![Point { x: 95.0, y: 50.0 }, Point { x: 5.0, y: 50.0 }]);
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("A".to_string(), mk_rect(0.0, 40.0, 20.0, 20.0));
        node_rects.insert("B".to_string(), mk_rect(80.0, 40.0, 20.0, 20.0));

        let bridge = build_reciprocal_gutter_bridge(
            &req_rel, &ret_rel, &req_route, &ret_route,
            &node_rects, "top", BRIDGE_GUTTER_CLEARANCE,
        ).expect("bridge should succeed");

        // request goes top side: surfYa = ra.y = 40, surfYb = rb.y = 40
        // edge = min(40,40) - 14 = 26, laneReq=26, laneRet=26-14=12
        assert_eq!(bridge.request.points[0], Point { x: 12.0, y: 40.0 });
        assert_eq!(bridge.request.points[1], Point { x: 12.0, y: 26.0 });
        assert_eq!(bridge.request.points[2], Point { x: 88.0, y: 26.0 });
        assert_eq!(bridge.request.points[3], Point { x: 88.0, y: 40.0 });
        assert_eq!(bridge.ret.points[0], Point { x: 92.0, y: 40.0 });
        assert_eq!(bridge.ret.points[1], Point { x: 92.0, y: 12.0 });
        assert_eq!(bridge.ret.points[2], Point { x: 8.0, y: 12.0 });
        assert_eq!(bridge.ret.points[3], Point { x: 8.0, y: 40.0 });
    }

    // -----------------------------------------------------------------------
    // surfaceSpacingCost
    // -----------------------------------------------------------------------

    #[test]
    fn surface_spacing_cost_zero_when_no_cramping() {
        // [10, 20] in length 40: all gaps >= MIN_LEGIBLE_GAP=4 → cost = 0
        assert_eq!(surface_spacing_cost(&[10.0, 20.0], 40.0), 0.0);
    }

    #[test]
    fn surface_spacing_cost_cramped() {
        // cramped_units([5,7,10],20) = 3.0; cost = 3 * 120 = 360
        assert_eq!(surface_spacing_cost(&[5.0, 7.0, 10.0], 20.0), 360.0);
    }
}
