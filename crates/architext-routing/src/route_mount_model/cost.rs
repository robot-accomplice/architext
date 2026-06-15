//! Cost measurement and surface analysis functions.
//!
//! Covers: excess_length, dogleg_count, intent_mismatch_count,
//! route_intersections, strict_crossing_count, surface_cramped_units,
//! surface_spacing_cost, surfaces_of, mount_cost_factors, mount_assignment_cost,
//! apply_offset_with_match.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::model::{Point, Rect};
use crate::route_constants::{rect_center, MOUNT_COST, MIN_LEGIBLE_GAP};
use crate::route_edges::{
    axis_aligned_segments, endpoint_side, offset_endpoint_route,
    route_collides_with_non_endpoints, route_has_endpoint_traversal,
    shared_segment_length, side_needs_post_selection_centering, RouteData,
};
use crate::route_geometry::shallow_jog_count;
use crate::route_ports::surface_capacity;

use super::helpers::{
    extract_rects, is_straight_facing, make_relationship,
    make_route_input, node_gap_length, point_key, route_length, side_normal, weighted_mount_cost,
};
use super::types::{
    MountCostFactors, MountInput, MountRect, MountRelationship, MountTarget, SurfaceInfo,
};

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
// surfaceCrampedUnits / surfaceSpacingCost
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

/// Port of JS `surfaceSpacingCost(positions, length)`.
pub fn surface_spacing_cost(positions: &[f64], length: f64) -> f64 {
    surface_cramped_units(positions, length) * MOUNT_COST.cramped
}

// ---------------------------------------------------------------------------
// movableEndpoints (private) / surfacesOf
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
// mountCostFactors / mountAssignmentCost
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
