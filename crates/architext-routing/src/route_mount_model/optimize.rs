//! Optimization passes: clear_elbows, route_unjustified_non_facing,
//! reciprocal_parallel_moves, optimize_mount_assignments,
//! realign_facing_endpoints, center_solo_reciprocal_pair_surfaces.

use indexmap::IndexMap;

use crate::js_compat::{js_default_sort_cmp, js_locale_compare};
use crate::model::Point;
use crate::route_constants::{
    BRIDGE_GUTTER_CLEARANCE, BRIDGE_LANE_GAP, BRIDGE_MAX_LANES, MIN_LEGIBLE_GAP,
    MOUNT_MAX_ITERS, RECIPROCAL_PARALLEL_OFFSET,
};
use crate::route_edges::{
    alternate_middle_dogleg_routes, aligned_facing_endpoint_route, endpoint_side,
    endpoint_spread_offset, offset_endpoint_route, offset_orthogonal_polyline,
    route_collides_with_non_endpoints, route_with_best_cleanup_candidate, route_with_points,
    side_endpoint_key, side_needs_post_selection_centering, RelationshipC1, Relationship,
    RouteData, RouteInput, RouteInputC1,
};
use crate::route_intent::{
    derive_route_intent, semantic_surface_options, DeriveRouteIntentInput,
    SemanticSurfaceOptionsInput, SidePair,
};

use super::cost::{mount_assignment_cost, mount_cost_factors};
use super::helpers::{
    extract_rects, facing_polish_cost, make_intent_relationship, make_relationship,
    make_route_input, no_hard_factor_worsening, respread_surfaces,
    restore_routes, snapshot_routes, weighted_mount_cost, SIDES,
};
use super::relief::{build_monotonic_staircase, build_reciprocal_gutter_bridge, try_side_moves};
use super::types::{BuildRouteForSides, MountInput, MountRect, MountRelationship};

// ---------------------------------------------------------------------------
// clearElbows (private)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(super) fn clear_elbows(
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
// routeNonFacingCount (private) / route_unjustified_non_facing
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
    let blocker_rects: Vec<crate::model::Rect> = input
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
    use std::collections::HashSet;
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
// SurfaceEndpointDesc (private, used by realign + distribution)
// ---------------------------------------------------------------------------

/// Endpoint descriptor used by orderedSurfaceEndpoints and its callers.
#[derive(Clone)]
pub(super) struct SurfaceEndpointDesc {
    pub relationship: MountRelationship,
    pub relationship_id: String,
    pub endpoint_index: usize,
    pub mount_rect: super::types::MountRect,
    pub side: String,
}

// ---------------------------------------------------------------------------
// oppositeEndpointProjection / oppositeRouteEndpointProjection (private)
// ---------------------------------------------------------------------------

/// Port of JS `oppositeEndpointProjection(endpoint, routeById, input)`.
#[allow(dead_code)]
fn opposite_endpoint_projection(
    relationship: &MountRelationship,
    endpoint_index: usize,
    side: &str,
    input: &MountInput<'_>,
) -> f64 {
    let opposite_node_id = if endpoint_index == 0 { &relationship.to } else { &relationship.from };
    if let Some(mount_rect) = input.node_rects.get(opposite_node_id) {
        let center = crate::route_constants::rect_center(&mount_rect.rect);
        if side == "top" || side == "bottom" { center.x } else { center.y }
    } else {
        0.0
    }
}

pub(super) fn opposite_route_endpoint_projection(
    relationship: &MountRelationship,
    endpoint_index: usize,
    side: &str,
    route_by_id: &IndexMap<String, RouteData>,
) -> f64 {
    let route = match route_by_id.get(&relationship.id) {
        Some(r) => r,
        None => return 0.0,
    };
    let opposite_point = if endpoint_index == 0 {
        route.points.last()
    } else {
        route.points.first()
    };
    match opposite_point {
        Some(p) => if side == "top" || side == "bottom" { p.x } else { p.y },
        None => 0.0,
    }
}

// ---------------------------------------------------------------------------
// realignFacingEndpoints
// ---------------------------------------------------------------------------

/// Port of JS `realignFacingEndpoints(routeById, relationshipById, input)`.
pub fn realign_facing_endpoints(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    // Build endpointGroups: key → Vec<relationshipId>
    let rects_plain = extract_rects(input.node_rects);
    let fixed_map: IndexMap<String, bool> = input.node_rects.iter()
        .filter(|(_, mr)| mr.fixed_ports)
        .map(|(k, _)| (k.clone(), true))
        .collect();
    let route_input = RouteInputC1 {
        visible_node_ids: input.visible_node_ids,
        node_rects: &rects_plain,
        fixed_ports: Some(&fixed_map),
    };

    let mut endpoint_groups: IndexMap<String, Vec<String>> = IndexMap::new();
    for (relationship_id, route) in route_by_id.iter() {
        let relationship = match rel_by_id.get(relationship_id) {
            Some(r) => r,
            None => continue,
        };
        if relationship.relationship_type != "flow" || route.points.is_empty() {
            continue;
        }
        let pairs: [(&str, &Point); 2] = [
            (relationship.from.as_str(), &route.points[0]),
            (relationship.to.as_str(), route.points.last().unwrap()),
        ];
        for (node_id, point) in &pairs {
            let mount_rect = match input.node_rects.get(*node_id) {
                Some(r) => r,
                None => continue,
            };
            if mount_rect.fixed_ports { continue; }
            let side = endpoint_side(&mount_rect.rect, point);
            if !side_needs_post_selection_centering(side) { continue; }
            let key = side_endpoint_key(node_id, side);
            endpoint_groups.entry(key).or_default().push(relationship_id.clone());
        }
    }

    // Snapshot all ids first to avoid borrow conflicts
    let rel_ids: Vec<String> = route_by_id.keys().cloned().collect();
    for relationship_id in &rel_ids {
        let relationship = match rel_by_id.get(relationship_id) {
            Some(r) => r,
            None => continue,
        };
        let route = match route_by_id.get(relationship_id) {
            Some(r) => r.clone(),
            None => continue,
        };

        let rel_c1 = RelationshipC1 {
            from: &relationship.from,
            to: &relationship.to,
            preferred_start_side: relationship.preferred_start_side.as_deref(),
            preferred_end_side: relationship.preferred_end_side.as_deref(),
        };

        let other_routes: Vec<RouteData> = route_by_id.iter()
            .filter(|(id, _)| *id != relationship_id)
            .map(|(_, r)| r.clone())
            .collect();

        let aligned_route = aligned_facing_endpoint_route(&route, &rel_c1, &route_input, &endpoint_groups);
        let candidates: Vec<Option<RouteData>> = {
            let mut c = Vec::new();
            c.push(Some(route.clone()));
            c.push(Some(aligned_route.clone()));
            for alt in alternate_middle_dogleg_routes(&route) { c.push(Some(alt)); }
            for alt in alternate_middle_dogleg_routes(&aligned_route) { c.push(Some(alt)); }
            c
        };
        let simple_rel = Relationship { from: &relationship.from, to: &relationship.to };
        let plain_input = RouteInput { visible_node_ids: input.visible_node_ids, node_rects: &rects_plain };
        if let Some(best) = route_with_best_cleanup_candidate(&candidates, &other_routes, &simple_rel, &plain_input) {
            route_by_id.insert(relationship_id.clone(), best.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// centerSoloReciprocalPairSurfaces
// ---------------------------------------------------------------------------

/// Port of JS `centerSoloReciprocalPairSurfaces(routeById, relationshipById, input)`.
pub fn center_solo_reciprocal_pair_surfaces(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    // Count how many flow endpoints sit on each node face.
    let mut surface_counts: IndexMap<String, usize> = IndexMap::new();
    for (relationship_id, route) in route_by_id.iter() {
        let relationship = match rel_by_id.get(relationship_id) {
            Some(r) => r,
            None => continue,
        };
        if relationship.relationship_type != "flow" || route.points.is_empty() { continue; }
        let pairs: [(&str, usize); 2] = [
            (&relationship.from, 0),
            (&relationship.to, route.points.len() - 1),
        ];
        for (node_id, ei) in &pairs {
            let mount_rect = match input.node_rects.get(*node_id) { Some(r) => r, None => continue };
            if mount_rect.fixed_ports { continue; }
            let point = if *ei == 0 { &route.points[0] } else { route.points.last().unwrap() };
            let side = endpoint_side(&mount_rect.rect, point);
            if !side_needs_post_selection_centering(side) { continue; }
            let key = side_endpoint_key(node_id, side);
            *surface_counts.entry(key).or_insert(0) += 1;
        }
    }

    // Group relationships by unordered node pair (NUL-separated, JS-sort order).
    let mut by_node_pair: IndexMap<String, Vec<String>> = IndexMap::new();
    for relationship in rel_by_id.values() {
        if relationship.relationship_type != "flow" || !route_by_id.contains_key(&relationship.id) { continue; }
        let mut parts = [relationship.from.as_str(), relationship.to.as_str()];
        parts.sort_by(|a, b| js_default_sort_cmp(a, b));
        let key = format!("{}\u{0000}{}", parts[0], parts[1]);
        by_node_pair.entry(key).or_default().push(relationship.id.clone());
    }

    for group_ids in by_node_pair.values() {
        if group_ids.len() != 2 { continue; }
        let rel_a = match rel_by_id.get(&group_ids[0]) { Some(r) => r, None => continue };
        let rel_b = match rel_by_id.get(&group_ids[1]) { Some(r) => r, None => continue };
        // Must be a true reciprocal pair (a.from==b.to && a.to==b.from).
        if rel_a.from != rel_b.to || rel_a.to != rel_b.from { continue; }

        // Collect solo-pair-surface endpoints for both routes.
        struct CsoloTarget { relationship_id: String, endpoint_index: usize, mount_rect: MountRect, side: String, key: String }
        let mut targets: Vec<CsoloTarget> = Vec::new();
        for relationship_id in &group_ids[..2] {
            let relationship = match rel_by_id.get(relationship_id) { Some(r) => r, None => continue };
            let route = match route_by_id.get(relationship_id) { Some(r) => r, None => continue };
            let pairs: [(&str, usize); 2] = [
                (&relationship.from, 0),
                (&relationship.to, route.points.len() - 1),
            ];
            for (node_id, ei) in &pairs {
                let mount_rect = match input.node_rects.get(*node_id) { Some(r) => r, None => continue };
                if mount_rect.fixed_ports { continue; }
                let point = if *ei == 0 { &route.points[0] } else { route.points.last().unwrap() };
                let side = endpoint_side(&mount_rect.rect, point);
                if !side_needs_post_selection_centering(side) { continue; }
                let key = side_endpoint_key(node_id, side);
                if surface_counts.get(&key).copied().unwrap_or(0) != 2 { continue; }
                targets.push(CsoloTarget {
                    relationship_id: relationship_id.clone(),
                    endpoint_index: *ei,
                    mount_rect: mount_rect.clone(),
                    side: side.to_string(),
                    key,
                });
            }
        }
        if targets.is_empty() { continue; }

        // Snapshot routes for guard.
        let affected_ids: Vec<String> = {
            let mut seen = IndexMap::new();
            for t in &targets { seen.entry(t.relationship_id.clone()).or_insert(()); }
            seen.into_keys().collect()
        };
        let saved: Vec<(String, RouteData)> = affected_ids.iter()
            .filter_map(|id| route_by_id.get(id).map(|r| (id.clone(), r.clone())))
            .collect();
        let before_bends: usize = saved.iter().map(|(_, r)| r.bends).sum();

        // Group targets by surface key and re-space each.
        let mut by_surface: IndexMap<String, Vec<usize>> = IndexMap::new();
        for (i, target) in targets.iter().enumerate() {
            by_surface.entry(target.key.clone()).or_default().push(i);
        }
        for surface_indices in by_surface.values() {
            if surface_indices.len() < 2 { continue; }
            let side = &targets[surface_indices[0]].side;
            let axis = if side == "left" || side == "right" { "y" } else { "x" };

            // Collect current mount values; sort by mount ascending.
            let mut order: Vec<(f64, usize)> = surface_indices.iter().map(|&i| {
                let t = &targets[i];
                let route = route_by_id.get(&t.relationship_id).unwrap();
                let point = if t.endpoint_index == 0 { &route.points[0] } else { route.points.last().unwrap() };
                let mount = if axis == "y" { point.y } else { point.x };
                (mount, i)
            }).collect();
            order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

            for (sort_pos, (_, ti)) in order.iter().enumerate() {
                let t = &targets[*ti];
                let route = route_by_id.get(&t.relationship_id).unwrap().clone();
                let offset = endpoint_spread_offset(sort_pos, order.len(), &t.mount_rect.rect, &t.side);
                let new_route = offset_endpoint_route(&route, t.endpoint_index, &t.mount_rect.rect, &t.side, offset);
                route_by_id.insert(t.relationship_id.clone(), new_route);
            }
        }

        let after_bends: usize = affected_ids.iter()
            .filter_map(|id| route_by_id.get(id))
            .map(|r| r.bends)
            .sum();
        let rects_plain = extract_rects(input.node_rects);
        let route_input_plain = make_route_input(input.visible_node_ids, &rects_plain);
        let collides = affected_ids.iter().any(|id| {
            let route = route_by_id.get(id).unwrap();
            let rel = rel_by_id.get(id).unwrap();
            let simple_rel = Relationship { from: &rel.from, to: &rel.to };
            route_collides_with_non_endpoints(route, &simple_rel, &route_input_plain)
        });
        if collides || after_bends > before_bends {
            for (id, route) in &saved { route_by_id.insert(id.clone(), route.clone()); }
        }
    }
}
