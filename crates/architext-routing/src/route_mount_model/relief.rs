//! Surface relief, gutter bridge, and monotonic staircase construction.
//!
//! Covers: try_side_moves, relief_candidate_ids, reciprocal_pairs,
//! reciprocal_crossing_pairs, relieve_crowded_surfaces,
//! build_reciprocal_gutter_bridge, build_monotonic_staircase.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::js_compat::js_locale_compare;
use crate::model::Point;
use crate::route_constants::{BRIDGE_LANE_GAP, BRIDGE_MOUNT_OFFSET};
use crate::route_edges::{endpoint_side, route_collides_with_non_endpoints, route_with_points, RouteData};
use crate::route_ports::surface_capacity;

use super::cost::{mount_assignment_cost, route_intersections, surfaces_of};
use super::helpers::{
    extract_rects, ideal_facing_side, make_relationship, make_route_input, reciprocal_pairs_inner,
    respread_surfaces, restore_routes, side_faces_partner, snapshot_routes, SIDES,
};
use super::types::{
    BuildRouteForSides, GutterBridge, MountInput, MountRect, MountRelationship, ReliefResult,
};

// ---------------------------------------------------------------------------
// trySideMoves (private)
// ---------------------------------------------------------------------------

pub(super) fn try_side_moves(
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
// reciprocalPairs / reciprocalCrossingPairs (private)
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
    reciprocal_pairs_inner(route_by_id, rel_by_id)
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
         rect: &crate::model::Rect,
         partner_rect: &crate::model::Rect,
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
    let clamp_x = |rect: &crate::model::Rect, x: f64| -> f64 {
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
