//! Reciprocal-parallel, surface-reorder, and endpoint-recenter passes (Pass C4).
//!
//! Ported from `viewer/src/routing/routeEdges.js`:
//! - `reduceCrossingsBySurfaceSwaps` (L1602): bubble-sort crossing reduction by swapping
//!   mount offsets on shared surfaces.
//! - `routeReciprocalPairsParallel`  (L1809): pin return legs parallel to their request.
//! - `reorderSharedSurfaceMounts`    (L1849): sort mounts so order matches opposite-centre order.
//! - `recenterSingletonSideEndpoints`(L1906): centre the single edge on each face.
//! - `spreadSharedSideEndpoints`     (L981):  spread multiple edges evenly across each face.
//! - `orderGutterLanesByTarget`      (L1708): push the farthest-reaching edge to the outermost lane.
//!
//! All of these need `MountInput` (rects with `fixedPorts`); the two functions that build
//! a fresh `relationshipById` from `input.relationships` (`recenter*` and `spread*`)
//! instead receive an explicit `rel_by_id` parameter for cleaner Rust ownership.
//!
//! Placement rationale: every function here reads `MountRect.fixed_ports`.  That type
//! lives in `route_mount_model::types`, so these functions belong here rather than in
//! `route_edges` (which has no `fixedPorts` concept).

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::js_compat::js_locale_compare;
use crate::model::{Point, Rect};
use crate::route_constants::{rect_center, RECIPROCAL_PARALLEL_OFFSET};
use crate::route_edges::{
    aligned_fixed_port_route, alternate_middle_dogleg_routes,
    collapse_aligned_opposing_surface_route, endpoint_side, endpoint_spread_offset,
    offset_endpoint_route, offset_orthogonal_polyline,
    recentered_endpoint_route_with_anchors, recentered_without_new_shared_segments,
    route_collides_with_non_endpoints,
    route_with_best_cleanup_candidate, route_with_points,
    side_endpoint_key, side_needs_post_selection_centering, RelationshipC1, Relationship,
    RouteData, RouteInput, RouteInputC1,
};
use crate::route_edges::crossings::{crossings_between, crossings_involving, gutter_lane_of};
use crate::route_mount_model::distribution::shared_segment_count_involving;

use super::helpers::{extract_rects, make_route_input};
use super::optimize::{opposite_route_endpoint_projection, SurfaceEndpointDesc};
use super::types::{MountInput, MountRect, MountRelationship};

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Build a plain `IndexMap<String, Rect>` from `MountInput.node_rects`.
fn plain_rects(input: &MountInput<'_>) -> IndexMap<String, Rect> {
    extract_rects(input.node_rects)
}

/// Build the minimal `RouteInput` needed for collision checks.
fn plain_route_input<'a>(
    visible: &'a [String],
    rects: &'a IndexMap<String, Rect>,
) -> RouteInput<'a> {
    make_route_input(visible, rects)
}

/// Convenience: get the `MountRect` for a node, returning `None` if absent.
fn get_mount_rect<'a>(
    node_id: &str,
    node_rects: &'a IndexMap<String, MountRect>,
) -> Option<&'a MountRect> {
    node_rects.get(node_id)
}

// ---------------------------------------------------------------------------
// crossingsTouching (private closure equivalent)
// ---------------------------------------------------------------------------

/// Total crossings between every pair in `routeById` where at least one member
/// appears in `ids`.  Mirrors the JS `crossingsTouching(ids)` inner closure in
/// `straightenSelfCrossingPairs` / `mirrorSelfCrossingBundles`.
#[allow(dead_code)]
fn crossings_touching(
    route_by_id: &IndexMap<String, RouteData>,
    ids: &[String],
) -> usize {
    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let entries: Vec<(&String, &RouteData)> = route_by_id.iter().collect();
    let mut total = 0usize;
    for i in 0..entries.len() {
        for j in i + 1..entries.len() {
            let (id_i, route_i) = entries[i];
            let (id_j, route_j) = entries[j];
            if !id_set.contains(id_i.as_str()) && !id_set.contains(id_j.as_str()) {
                continue;
            }
            total += crossings_between(route_i, route_j);
        }
    }
    total
}

// ---------------------------------------------------------------------------
// reduceCrossingsBySurfaceSwaps (L1602)
// ---------------------------------------------------------------------------

/// Port of JS `reduceCrossingsBySurfaceSwaps(routeById, relationshipById, input)`.
///
/// Local-search (bubble-sort) crossing reduction: when two edges cross AND share a
/// mount surface, try swapping their mount offsets on that surface; keep the swap
/// only if it reduces total crossings without colliding with a node.  Bounded to
/// 12 passes.  No-ops when `ids.len() < 2` or `> 80`.
pub fn reduce_crossings_by_surface_swaps(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    let ids: Vec<String> = route_by_id.keys().cloned().collect();
    if ids.len() < 2 || ids.len() > 80 {
        return;
    }
    let rects = plain_rects(input);
    let route_input = plain_route_input(input.visible_node_ids, &rects);

    // total_crossings: count all pairwise crossings in current state.
    let total_crossings = |rbd: &IndexMap<String, RouteData>| -> usize {
        let mut total = 0usize;
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                if let (Some(ri), Some(rj)) = (rbd.get(&ids[i]), rbd.get(&ids[j])) {
                    total += crossings_between(ri, rj);
                }
            }
        }
        total
    };

    // surfaceEndpoints: movable endpoints for a given relationship.
    struct SurfaceEp {
        node: String,
        side: String,
        endpoint_index: usize,
        rect: Rect,
    }
    let surface_endpoints =
        |relationship_id: &str, rbd: &IndexMap<String, RouteData>| -> Vec<SurfaceEp> {
            let relationship = match rel_by_id.get(relationship_id) {
                Some(r) => r,
                None => return vec![],
            };
            let route = match rbd.get(relationship_id) {
                Some(r) if !r.points.is_empty() => r,
                _ => return vec![],
            };
            let mut out = Vec::new();
            let from_mr = get_mount_rect(&relationship.from, input.node_rects);
            let to_mr = get_mount_rect(&relationship.to, input.node_rects);
            let start_side = from_mr.map(|mr| endpoint_side(&mr.rect, &route.points[0])).unwrap_or("");
            let end_side = to_mr.map(|mr| endpoint_side(&mr.rect, route.points.last().unwrap())).unwrap_or("");
            if let Some(mr) = from_mr {
                if !mr.fixed_ports && side_needs_post_selection_centering(start_side) {
                    out.push(SurfaceEp {
                        node: relationship.from.clone(),
                        side: start_side.to_string(),
                        endpoint_index: 0,
                        rect: mr.rect.clone(),
                    });
                }
            }
            if let Some(mr) = to_mr {
                if !mr.fixed_ports && side_needs_post_selection_centering(end_side) {
                    out.push(SurfaceEp {
                        node: relationship.to.clone(),
                        side: end_side.to_string(),
                        endpoint_index: route.points.len() - 1,
                        rect: mr.rect.clone(),
                    });
                }
            }
            out
        };

    let mut improved = true;
    for _pass in 0..12 {
        if !improved {
            break;
        }
        improved = false;
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                let a = ids[i].clone();
                let b = ids[j].clone();
                // Quick check: skip pairs that don't cross.
                {
                    let ra = match route_by_id.get(&a) { Some(r) => r, None => continue };
                    let rb = match route_by_id.get(&b) { Some(r) => r, None => continue };
                    if crossings_between(ra, rb) == 0 {
                        continue;
                    }
                }
                let eps_a = surface_endpoints(&a, route_by_id);
                let eps_b = surface_endpoints(&b, route_by_id);
                for pa in &eps_a {
                    for pb in &eps_b {
                        if pa.node != pb.node || pa.side != pb.side {
                            continue;
                        }
                        // Same surface — try swapping offsets.
                        let axis = if pa.side == "left" || pa.side == "right" { "y" } else { "x" };
                        let center = if axis == "y" {
                            pa.rect.y + pa.rect.height / 2.0
                        } else {
                            pa.rect.x + pa.rect.width / 2.0
                        };
                        let (point_a, point_b) = {
                            let ra = route_by_id.get(&a).unwrap();
                            let rb = route_by_id.get(&b).unwrap();
                            let pa_pt = if pa.endpoint_index == 0 { &ra.points[0] } else { ra.points.last().unwrap() };
                            let pb_pt = if pb.endpoint_index == 0 { &rb.points[0] } else { rb.points.last().unwrap() };
                            (pa_pt.clone(), pb_pt.clone())
                        };
                        let offset_a = if axis == "y" { point_a.y - center } else { point_a.x - center };
                        let offset_b = if axis == "y" { point_b.y - center } else { point_b.x - center };
                        if (offset_a - offset_b).abs() < 0.5 {
                            continue;
                        }
                        let before = total_crossings(route_by_id);
                        let route_a = route_by_id.get(&a).unwrap().clone();
                        let route_b = route_by_id.get(&b).unwrap().clone();
                        let swapped_a = offset_endpoint_route(&route_a, pa.endpoint_index, &pa.rect, &pa.side, offset_b);
                        let swapped_b = offset_endpoint_route(&route_b, pb.endpoint_index, &pb.rect, &pb.side, offset_a);
                        route_by_id.insert(a.clone(), swapped_a.clone());
                        route_by_id.insert(b.clone(), swapped_b.clone());
                        let rel_a = rel_by_id.get(&a).map(|r| Relationship { from: &r.from, to: &r.to });
                        let rel_b = rel_by_id.get(&b).map(|r| Relationship { from: &r.from, to: &r.to });
                        let collides = rel_a.as_ref().map(|r| route_collides_with_non_endpoints(&swapped_a, r, &route_input)).unwrap_or(false)
                            || rel_b.as_ref().map(|r| route_collides_with_non_endpoints(&swapped_b, r, &route_input)).unwrap_or(false);
                        if !collides && total_crossings(route_by_id) < before {
                            improved = true;
                        } else {
                            route_by_id.insert(a.clone(), route_a);
                            route_by_id.insert(b.clone(), route_b);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// routeReciprocalPairsParallel (L1809)
// ---------------------------------------------------------------------------

/// Port of JS `routeReciprocalPairsParallel(routeById, relationshipById, input, restrictIds)`.
///
/// Routes the return leg of each A↔B reciprocal pair as a constant perpendicular
/// offset of the reversed request so the two never cross.  The original return is
/// kept if neither ±12 px offset avoids a node collision.
///
/// `restrict_ids`: when `Some(set)`, only re-separate pairs that have at least one
/// id in the set (the post-relief restricted replay).  `None` processes all pairs.
pub fn route_reciprocal_pairs_parallel(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    restrict_ids: Option<&HashSet<String>>,
) {
    let rects = plain_rects(input);
    let route_input = plain_route_input(input.visible_node_ids, &rects);

    // Build: key = sorted([from,to]).join("\0") → Vec<relationship>
    let mut by_node_pair: IndexMap<String, Vec<&MountRelationship>> = IndexMap::new();
    for relationship in rel_by_id.values() {
        if relationship.relationship_type != "flow" {
            continue;
        }
        if !route_by_id.contains_key(&relationship.id) {
            continue;
        }
        let mut parts = [relationship.from.as_str(), relationship.to.as_str()];
        parts.sort();
        let key = format!("{}\0{}", parts[0], parts[1]);
        by_node_pair.entry(key).or_default().push(relationship);
    }

    for group in by_node_pair.values() {
        if group.len() != 2 {
            continue;
        }
        let (a, b) = (group[0], group[1]);
        // Confirm a true reciprocal pair (one direction each).
        if a.from != b.to || a.to != b.from {
            continue;
        }
        // Post-relief restriction: skip pairs not touched by relief.
        if let Some(ids) = restrict_ids {
            if !ids.contains(&a.id) && !ids.contains(&b.id) {
                continue;
            }
        }
        // Request = lower displayIndex.
        let (request, ret) = if (a.display_index) <= (b.display_index) {
            (a, b)
        } else {
            (b, a)
        };
        let request_route = match route_by_id.get(&request.id) {
            Some(r) if !r.points.is_empty() => r.clone(),
            _ => continue,
        };
        let return_route = match route_by_id.get(&ret.id) {
            Some(r) if !r.points.is_empty() => r.clone(),
            _ => continue,
        };

        let reversed: Vec<Point> = request_route.points.iter().cloned().rev().collect();
        let ret_rel = Relationship { from: &ret.from, to: &ret.to };

        for delta in [RECIPROCAL_PARALLEL_OFFSET, -RECIPROCAL_PARALLEL_OFFSET] {
            let candidate_points = offset_orthogonal_polyline(&reversed, delta);
            let candidate = route_with_points(&return_route, candidate_points, None);
            if !route_collides_with_non_endpoints(&candidate, &ret_rel, &route_input) {
                route_by_id.insert(ret.id.clone(), candidate);
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// reorderSharedSurfaceMounts (L1849)
// ---------------------------------------------------------------------------

/// Port of JS `reorderSharedSurfaceMounts(routeById, relationshipById, input)`.
///
/// Post-pass: re-spreads each shared mount surface so mount-point order matches
/// opposite-node-centre order.  Mount order that disagrees with destination order
/// forces a crossing; sorting by the opposite endpoint's final position removes
/// those same-surface crossings.  Runs 2 passes.
pub fn reorder_shared_surface_mounts(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    let axis_for = |side: &str| -> &'static str {
        if side == "left" || side == "right" { "y" } else { "x" }
    };

    for _pass in 0..2 {
        // Build groups: sideEndpointKey → list of endpoint descriptors.
        let mut groups: IndexMap<String, Vec<(String, usize, Rect, String)>> = IndexMap::new();
        // (relationship_id, endpoint_index, rect, side)

        for (relationship_id, route) in route_by_id.iter() {
            let relationship = match rel_by_id.get(relationship_id) {
                Some(r) => r,
                None => continue,
            };
            if relationship.relationship_type != "flow" || route.points.is_empty() {
                continue;
            }
            let register = |node_id: &str, endpoint_index: usize,
                             groups: &mut IndexMap<String, Vec<(String, usize, Rect, String)>>| {
                let mr = match input.node_rects.get(node_id) {
                    Some(r) => r,
                    None => return,
                };
                let point = if endpoint_index == 0 {
                    &route.points[0]
                } else {
                    route.points.last().unwrap()
                };
                let side = endpoint_side(&mr.rect, point);
                if mr.fixed_ports || !side_needs_post_selection_centering(side) {
                    return;
                }
                let key = side_endpoint_key(node_id, side);
                groups.entry(key).or_default().push((
                    relationship_id.clone(),
                    endpoint_index,
                    mr.rect.clone(),
                    side.to_string(),
                ));
            };
            let last_idx = route.points.len() - 1;
            // Need to clone since we call register twice.
            let from = relationship.from.clone();
            let to = relationship.to.clone();
            register(&from, 0, &mut groups);
            register(&to, last_idx, &mut groups);
        }

        let mut changed = false;
        for endpoints in groups.values() {
            if endpoints.len() < 2 {
                continue;
            }
            let axis = axis_for(&endpoints[0].3);

            // Build enriched descriptors.
            struct Enriched {
                relationship_id: String,
                endpoint_index: usize,
                rect: Rect,
                side: String,
                mount: f64,         // current mount coord along axis
                opposite_center: f64,
                display_index: i64,
            }

            let enriched: Vec<Enriched> = endpoints
                .iter()
                .filter_map(|(rel_id, ep_idx, rect, side)| {
                    let route = route_by_id.get(rel_id)?;
                    let relationship = rel_by_id.get(rel_id)?;
                    let mount_point = if *ep_idx == 0 {
                        &route.points[0]
                    } else {
                        route.points.last()?
                    };
                    let opposite_node_id = if *ep_idx == 0 { &relationship.to } else { &relationship.from };
                    let opposite_rect = input.node_rects.get(opposite_node_id);
                    let opposite_center = opposite_rect.map(|mr| {
                        if axis == "y" {
                            mr.rect.y + mr.rect.height / 2.0
                        } else {
                            mr.rect.x + mr.rect.width / 2.0
                        }
                    }).unwrap_or(0.0);
                    let mount = if axis == "y" { mount_point.y } else { mount_point.x };
                    Some(Enriched {
                        relationship_id: rel_id.clone(),
                        endpoint_index: *ep_idx,
                        rect: rect.clone(),
                        side: side.clone(),
                        mount,
                        opposite_center,
                        display_index: relationship.display_index,
                    })
                })
                .collect();

            if enriched.len() < 2 {
                continue;
            }

            let order = |list: &[&Enriched]| -> String {
                list.iter().map(|e| e.relationship_id.as_str()).collect::<Vec<_>>().join("|")
            };

            // desired: sort by opposite_center, then display_index, then id.localeCompare
            let mut desired_refs: Vec<&Enriched> = enriched.iter().collect();
            desired_refs.sort_by(|a, b| {
                a.opposite_center.partial_cmp(&b.opposite_center)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.display_index.cmp(&b.display_index))
                    .then_with(|| js_locale_compare(&a.relationship_id, &b.relationship_id))
            });

            // current: sort by mount coord, then id.localeCompare
            let mut current_refs: Vec<&Enriched> = enriched.iter().collect();
            current_refs.sort_by(|a, b| {
                a.mount.partial_cmp(&b.mount)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| js_locale_compare(&a.relationship_id, &b.relationship_id))
            });

            if order(&desired_refs) == order(&current_refs) {
                continue;
            }

            // Apply desired ordering: use endpointSpreadOffset for each position.
            for (index, e) in desired_refs.iter().enumerate() {
                if let Some(route) = route_by_id.get(&e.relationship_id).cloned() {
                    let offset = endpoint_spread_offset(index, desired_refs.len(), &e.rect, &e.side);
                    let new_route = offset_endpoint_route(&route, e.endpoint_index, &e.rect, &e.side, offset);
                    route_by_id.insert(e.relationship_id.clone(), new_route);
                }
            }
            changed = true;
        }

        if !changed {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// recenterSingletonSideEndpoints (L1906)
// ---------------------------------------------------------------------------

/// Port of JS `recenterSingletonSideEndpoints(plannedRawRoutes, input)`.
///
/// For every endpoint that is the SOLE edge on its node face, shifts it to the
/// face centre.  Returns a new `Vec<(String, RouteData)>` with updated routes.
pub fn recenter_singleton_side_endpoints(
    planned_raw_routes: &[(String, RouteData)],
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) -> Vec<(String, RouteData)> {
    // Build endpointCounts: sideEndpointKey → count.
    let mut endpoint_counts: IndexMap<String, usize> = IndexMap::new();
    let rects = plain_rects(input);
    let route_input = plain_route_input(input.visible_node_ids, &rects);

    for (relationship_id, route) in planned_raw_routes {
        let relationship = match rel_by_id.get(relationship_id) {
            Some(r) => r,
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        let from_mr = get_mount_rect(&relationship.from, input.node_rects);
        let to_mr = get_mount_rect(&relationship.to, input.node_rects);
        let start_side = from_mr.map(|mr| endpoint_side(&mr.rect, &route.points[0])).unwrap_or("");
        let end_side = to_mr.map(|mr| endpoint_side(&mr.rect, route.points.last().unwrap())).unwrap_or("");
        if side_needs_post_selection_centering(start_side) {
            let key = side_endpoint_key(&relationship.from, start_side);
            *endpoint_counts.entry(key).or_insert(0) += 1;
        }
        if side_needs_post_selection_centering(end_side) {
            let key = side_endpoint_key(&relationship.to, end_side);
            *endpoint_counts.entry(key).or_insert(0) += 1;
        }
    }

    // Build a mutable route_by_id for other-route lookups during recentering.
    let mut route_by_id: IndexMap<String, RouteData> = planned_raw_routes.iter().cloned().collect();

    planned_raw_routes
        .iter()
        .map(|(relationship_id, route)| {
            let relationship = match rel_by_id.get(relationship_id) {
                Some(r) => r,
                None => return (relationship_id.clone(), route.clone()),
            };
            if route.points.is_empty() {
                return (relationship_id.clone(), route.clone());
            }

            let mut current = route.clone();
            let mut points = current.points.clone();

            // From (start) endpoint.
            let from_mr = get_mount_rect(&relationship.from, input.node_rects);
            let start_side = from_mr.map(|mr| endpoint_side(&mr.rect, &points[0])).unwrap_or("");
            if let Some(mr) = from_mr {
                let key = side_endpoint_key(&relationship.from, start_side);
                if side_needs_post_selection_centering(start_side)
                    && endpoint_counts.get(&key).copied().unwrap_or(0) == 1
                {
                    let next_route = recentered_endpoint_route_with_anchors(
                        &current,
                        0,
                        &mr.rect,
                        start_side,
                        mr.side_anchors.as_ref(),
                    );
                    let other_routes: Vec<RouteData> = route_by_id
                        .iter()
                        .filter(|(id, _)| *id != relationship_id)
                        .map(|(_, r)| r.clone())
                        .collect();
                    let rel_simple = Relationship { from: &relationship.from, to: &relationship.to };
                    current = recentered_without_new_shared_segments(
                        &current,
                        next_route,
                        &other_routes,
                        Some((&rel_simple, &route_input)),
                    );
                    points = current.points.clone();
                }
            }

            // To (end) endpoint.
            let to_mr = get_mount_rect(&relationship.to, input.node_rects);
            let end_side = to_mr.map(|mr| endpoint_side(&mr.rect, points.last().unwrap())).unwrap_or("");
            if let Some(mr) = to_mr {
                let key = side_endpoint_key(&relationship.to, end_side);
                if side_needs_post_selection_centering(end_side)
                    && endpoint_counts.get(&key).copied().unwrap_or(0) == 1
                {
                    let last_idx = points.len() - 1;
                    let next_route = recentered_endpoint_route_with_anchors(
                        &current,
                        last_idx,
                        &mr.rect,
                        end_side,
                        mr.side_anchors.as_ref(),
                    );
                    let other_routes: Vec<RouteData> = route_by_id
                        .iter()
                        .filter(|(id, _)| *id != relationship_id)
                        .map(|(_, r)| r.clone())
                        .collect();
                    let rel_simple = Relationship { from: &relationship.from, to: &relationship.to };
                    current = recentered_without_new_shared_segments(
                        &current,
                        next_route,
                        &other_routes,
                        Some((&rel_simple, &route_input)),
                    );
                }
            }

            route_by_id.insert(relationship_id.clone(), current.clone());

            // Apply aligned_fixed_port_route to snap fixed-port endpoints.
            let rel_c1 = RelationshipC1 {
                from: &relationship.from,
                to: &relationship.to,
                preferred_start_side: relationship.preferred_start_side.as_deref(),
                preferred_end_side: relationship.preferred_end_side.as_deref(),
            };
            let fixed_map: IndexMap<String, bool> = input.node_rects.iter()
                .filter(|(_, mr)| mr.fixed_ports)
                .map(|(k, _)| (k.clone(), true))
                .collect();
            let route_input_c1 = RouteInputC1 {
                visible_node_ids: input.visible_node_ids,
                node_rects: &rects,
                fixed_ports: Some(&fixed_map),
            };
            let final_route = aligned_fixed_port_route(&current, &rel_c1, &route_input_c1);
            (relationship_id.clone(), final_route)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// spreadSharedSideEndpoints (L981)
// ---------------------------------------------------------------------------

/// Port of JS `spreadSharedSideEndpoints(plannedRawRoutes, input)`.
///
/// For every node face that hosts multiple endpoint mounts, spreads them evenly
/// using `orderedSurfaceEndpoints` ordering (opposite node centre, then
/// displayIndex, then opposite route endpoint, then id.localeCompare).
///
/// Returns a new `Vec<(String, RouteData)>` with updated routes.
pub fn spread_shared_side_endpoints(
    planned_raw_routes: &[(String, RouteData)],
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) -> Vec<(String, RouteData)> {
    let rects = plain_rects(input);
    let route_input = plain_route_input(input.visible_node_ids, &rects);
    let fixed_map: IndexMap<String, bool> = input.node_rects.iter()
        .filter(|(_, mr)| mr.fixed_ports)
        .map(|(k, _)| (k.clone(), true))
        .collect();
    let route_input_c1 = RouteInputC1 {
        visible_node_ids: input.visible_node_ids,
        node_rects: &rects,
        fixed_ports: Some(&fixed_map),
    };

    // Build endpointGroups: sideEndpointKey → Vec<SurfaceEndpointDesc>
    let mut endpoint_groups: IndexMap<String, Vec<SurfaceEndpointDesc>> = IndexMap::new();

    for (relationship_id, route) in planned_raw_routes {
        let relationship = match rel_by_id.get(relationship_id) {
            Some(r) => r,
            None => continue,
        };
        if route.points.is_empty() {
            continue;
        }
        if relationship.relationship_type != "flow" {
            continue;
        }

        // From endpoint.
        let from_mr = get_mount_rect(&relationship.from, input.node_rects);
        let start_side = from_mr.map(|mr| endpoint_side(&mr.rect, &route.points[0])).unwrap_or("");
        if let Some(mr) = from_mr {
            if !mr.fixed_ports && side_needs_post_selection_centering(start_side) {
                let key = side_endpoint_key(&relationship.from, start_side);
                endpoint_groups.entry(key).or_default().push(SurfaceEndpointDesc {
                    relationship: relationship.clone(),
                    relationship_id: relationship_id.clone(),
                    endpoint_index: 0,
                    mount_rect: mr.clone(),
                    side: start_side.to_string(),
                });
            }
        }

        // To endpoint.
        let to_mr = get_mount_rect(&relationship.to, input.node_rects);
        let end_side = to_mr.map(|mr| endpoint_side(&mr.rect, route.points.last().unwrap())).unwrap_or("");
        if let Some(mr) = to_mr {
            if !mr.fixed_ports && side_needs_post_selection_centering(end_side) {
                let key = side_endpoint_key(&relationship.to, end_side);
                endpoint_groups.entry(key).or_default().push(SurfaceEndpointDesc {
                    relationship: relationship.clone(),
                    relationship_id: relationship_id.clone(),
                    endpoint_index: route.points.len() - 1,
                    mount_rect: mr.clone(),
                    side: end_side.to_string(),
                });
            }
        }
    }

    let mut route_by_id: IndexMap<String, RouteData> = planned_raw_routes.iter().cloned().collect();

    for endpoints in endpoint_groups.values() {
        if endpoints.len() <= 1 {
            continue;
        }

        // orderedSurfaceEndpoints: sort by (oppositeEndpointProjection, displayIndex,
        // oppositeRouteEndpointProjection, relationshipId.localeCompare).
        let mut ordered = endpoints.to_vec();
        ordered.sort_by(|left, right| {
            let oep_l = opposite_endpoint_projection_mount(&left.relationship, left.endpoint_index, &left.side, input);
            let oep_r = opposite_endpoint_projection_mount(&right.relationship, right.endpoint_index, &right.side, input);
            oep_l.partial_cmp(&oep_r).unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| (left.relationship.display_index).cmp(&right.relationship.display_index))
                .then_with(|| {
                    let lp = opposite_route_endpoint_projection(&left.relationship, left.endpoint_index, &left.side, &route_by_id);
                    let rp = opposite_route_endpoint_projection(&right.relationship, right.endpoint_index, &right.side, &route_by_id);
                    lp.partial_cmp(&rp).unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| js_locale_compare(&left.relationship_id, &right.relationship_id))
        });

        for (index, endpoint) in ordered.iter().enumerate() {
            let route = match route_by_id.get(&endpoint.relationship_id) {
                Some(r) => r.clone(),
                None => continue,
            };
            let raw_offset = endpoint_spread_offset(
                index,
                ordered.len(),
                &endpoint.mount_rect.rect,
                &endpoint.side,
            );
            let offset_route = offset_endpoint_route(
                &route,
                endpoint.endpoint_index,
                &endpoint.mount_rect.rect,
                &endpoint.side,
                raw_offset,
            );

            // Optionally align the opposite endpoint if it is a singleton on a
            // left/right or top/bottom face pair.
            let opposite_index = if endpoint.endpoint_index == 0 {
                offset_route.points.len().saturating_sub(1)
            } else {
                0
            };
            let opposite_node_id = if endpoint.endpoint_index == 0 {
                &endpoint.relationship.to
            } else {
                &endpoint.relationship.from
            };
            let opposite_mr = get_mount_rect(opposite_node_id, input.node_rects);
            let opposite_side = opposite_mr.map(|mr| {
                endpoint_side(&mr.rect, &offset_route.points[opposite_index])
            }).unwrap_or("");
            let key_for_opp = side_endpoint_key(opposite_node_id, opposite_side);
            let opposite_endpoint_count = endpoint_groups.get(&key_for_opp).map(|v| v.len()).unwrap_or(0);
            let can_align_opposite = opposite_mr
                .map(|mr| !mr.fixed_ports)
                .unwrap_or(false)
                && side_needs_post_selection_centering(opposite_side)
                && opposite_endpoint_count <= 1;

            let aligned_route = if can_align_opposite {
                let opp_rect = &opposite_mr.unwrap().rect;
                let is_lr_ep = endpoint.side == "left" || endpoint.side == "right";
                let is_lr_opp = opposite_side == "left" || opposite_side == "right";
                let is_tb_ep = endpoint.side == "top" || endpoint.side == "bottom";
                let is_tb_opp = opposite_side == "top" || opposite_side == "bottom";
                let ep_idx_0 = endpoint.endpoint_index == 0;
                if is_lr_ep && is_lr_opp {
                    let align_offset = offset_route.points.get(if ep_idx_0 { 0 } else { offset_route.points.len().saturating_sub(1) }).map(|p| p.y).unwrap_or(0.0)
                        - rect_center(opp_rect).y;
                    offset_endpoint_route(&offset_route, opposite_index, opp_rect, opposite_side, align_offset)
                } else if is_tb_ep && is_tb_opp {
                    let align_offset = offset_route.points.get(if ep_idx_0 { 0 } else { offset_route.points.len().saturating_sub(1) }).map(|p| p.x).unwrap_or(0.0)
                        - rect_center(opp_rect).x;
                    offset_endpoint_route(&offset_route, opposite_index, opp_rect, opposite_side, align_offset)
                } else {
                    offset_route.clone()
                }
            } else {
                offset_route.clone()
            };

            // Determine first/last rect for collapseAlignedOpposingSurfaceRoute.
            let first_mr = get_mount_rect(&endpoint.relationship.from, input.node_rects);
            let last_mr = get_mount_rect(&endpoint.relationship.to, input.node_rects);
            let rel_c1 = RelationshipC1 {
                from: &endpoint.relationship.from,
                to: &endpoint.relationship.to,
                preferred_start_side: endpoint.relationship.preferred_start_side.as_deref(),
                preferred_end_side: endpoint.relationship.preferred_end_side.as_deref(),
            };
            let other_routes: Vec<RouteData> = route_by_id
                .iter()
                .filter(|(id, _)| **id != endpoint.relationship_id)
                .map(|(_, r)| r.clone())
                .collect();
            let simple_rel = Relationship {
                from: &endpoint.relationship.from,
                to: &endpoint.relationship.to,
            };

            let candidates: Vec<Option<RouteData>> = [&offset_route, &aligned_route]
                .iter()
                .flat_map(|candidate_route| {
                    let first_side = first_mr.map(|mr| endpoint_side(&mr.rect, &candidate_route.points[0])).unwrap_or("");
                    let last_side = last_mr.map(|mr| endpoint_side(&mr.rect, candidate_route.points.last().unwrap())).unwrap_or("");
                    let collapsed = collapse_aligned_opposing_surface_route(
                        candidate_route,
                        first_side,
                        last_side,
                        &rel_c1,
                        &route_input_c1,
                    );
                    let mut v: Vec<Option<RouteData>> = vec![Some(collapsed.clone())];
                    for alt in alternate_middle_dogleg_routes(&collapsed) {
                        v.push(Some(alt));
                    }
                    v
                })
                .collect();

            if let Some(best) = route_with_best_cleanup_candidate(&candidates, &other_routes, &simple_rel, &route_input) {
                route_by_id.insert(endpoint.relationship_id.clone(), best.clone());
            }
        }
    }

    planned_raw_routes
        .iter()
        .map(|(id, _)| {
            let r = route_by_id.get(id).cloned().unwrap_or_else(|| {
                planned_raw_routes.iter().find(|(i, _)| i == id).map(|(_, r)| r.clone()).unwrap()
            });
            (id.clone(), r)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// orderGutterLanesByTarget (L1708)
// ---------------------------------------------------------------------------

/// Port of JS `orderGutterLanesByTarget(routeById, relationshipById, input)`.
///
/// Gutter-lane order: farthest target → outermost lane.  When several edges
/// leave the same node face and run perpendicular gutters to targets at
/// different distances, the farthest-target edge must take the OUTERMOST lane
/// so it brackets OVER the shorter ones instead of slicing their stubs.
pub fn order_gutter_lanes_by_target(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    const LANE_GAP: f64 = 12.0;
    const FACE_PAD: f64 = 6.0;

    let rects = plain_rects(input);
    let route_input = plain_route_input(input.visible_node_ids, &rects);

    // Build faces: sideEndpointKey → { rect, side, members }
    struct FaceMember {
        relationship_id: String,
        endpoint_index: usize,
        route: RouteData,
        far: Option<Point>, // set in second pass
        reach: f64,
        lane: Option<f64>,
    }
    struct Face {
        rect: Rect,
        side: String,
        members: Vec<FaceMember>,
    }

    let mut faces: IndexMap<String, Face> = IndexMap::new();

    for (relationship_id, route) in route_by_id.iter() {
        let relationship = match rel_by_id.get(relationship_id) {
            Some(r) => r,
            None => continue,
        };
        if relationship.relationship_type != "flow" || route.points.is_empty() {
            continue;
        }
        for (node_id, endpoint_index) in
            [(&relationship.from, 0usize), (&relationship.to, route.points.len() - 1)]
        {
            let mr = match input.node_rects.get(node_id) {
                Some(r) => r,
                None => continue,
            };
            if mr.fixed_ports {
                continue;
            }
            let point = if endpoint_index == 0 {
                &route.points[0]
            } else {
                route.points.last().unwrap()
            };
            let side = endpoint_side(&mr.rect, point);
            if !side_needs_post_selection_centering(side) {
                continue;
            }
            let key = side_endpoint_key(node_id, side);
            let face = faces.entry(key).or_insert_with(|| Face {
                rect: mr.rect.clone(),
                side: side.to_string(),
                members: vec![],
            });
            face.members.push(FaceMember {
                relationship_id: relationship_id.clone(),
                endpoint_index,
                route: route.clone(),
                far: None,
                reach: 0.0,
                lane: None,
            });
        }
    }

    for face in faces.values_mut() {
        if face.members.len() < 3 {
            continue;
        }
        let along = if face.side == "left" || face.side == "right" { "y" } else { "x" };
        let perp = if along == "y" { "x" } else { "y" };
        let face_edge: f64 = match face.side.as_str() {
            "right" => face.rect.x + face.rect.width,
            "left" => face.rect.x,
            "bottom" => face.rect.y + face.rect.height,
            _ => face.rect.y, // top
        };
        let away: f64 = if face.side == "right" || face.side == "bottom" { 1.0 } else { -1.0 };
        let face_lo = if along == "y" { face.rect.y } else { face.rect.x };
        let face_hi = if along == "y" { face.rect.y + face.rect.height } else { face.rect.x + face.rect.width };
        let face_center = (face_lo + face_hi) / 2.0;

        // Compute reach + lane for each member.
        for member in &mut face.members {
            let far = if member.endpoint_index == 0 {
                member.route.points.last().cloned()
            } else {
                member.route.points.first().cloned()
            };
            let far_pt = far.unwrap_or(Point { x: 0.0, y: 0.0 });
            let far_along = if along == "y" { far_pt.y } else { far_pt.x };
            member.reach = (far_along - face_center).abs();
            member.lane = gutter_lane_of(&member.route, perp, along);
            member.far = Some(far_pt);
        }

        // Filter to reachable (have a lane + reach > 0).
        let reachable_indices: Vec<usize> = face
            .members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.lane.is_some() && m.reach > 0.0)
            .map(|(i, _)| i)
            .collect();
        if reachable_indices.len() < 2 {
            continue;
        }

        // Find the member with highest reach.
        let farthest_idx = *reachable_indices
            .iter()
            .max_by(|&&a, &&b| face.members[a].reach.partial_cmp(&face.members[b].reach).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        let siblings: Vec<usize> = reachable_indices.iter().filter(|&&i| i != farthest_idx).cloned().collect();
        let outermost_sibling_lane = siblings
            .iter()
            .map(|&i| face.members[i].lane.unwrap() * away)
            .fold(f64::NEG_INFINITY, f64::max);

        let farthest_lane = face.members[farthest_idx].lane.unwrap();
        if farthest_lane * away >= outermost_sibling_lane {
            continue; // already outermost
        }
        let farthest_id = face.members[farthest_idx].relationship_id.clone();
        if crossings_involving(route_by_id, &farthest_id) == 0 {
            continue; // not actually crossing
        }

        let new_lane = face_edge + (outermost_sibling_lane.abs() - face_edge.abs() + LANE_GAP) * away;
        let far_pt = face.members[farthest_idx].far.clone().unwrap();
        let far_along = if along == "y" { far_pt.y } else { far_pt.x };
        let comb_dir = if far_along - face_center > 0.0 { 1.0 } else if far_along - face_center < 0.0 { -1.0 } else { 1.0 };
        let clearing_mount = if comb_dir > 0.0 { face_lo + FACE_PAD } else { face_hi - FACE_PAD };

        let (face_corner, elbow, turn) = if along == "y" {
            (
                Point { x: face_edge, y: clearing_mount },
                Point { x: new_lane, y: clearing_mount },
                Point { x: new_lane, y: far_pt.y },
            )
        } else {
            (
                Point { x: clearing_mount, y: face_edge },
                Point { x: clearing_mount, y: new_lane },
                Point { x: far_pt.x, y: new_lane },
            )
        };
        let far_end = Point { x: far_pt.x, y: far_pt.y };
        let sequence = vec![face_corner, elbow, turn, far_end];
        let ep_idx = face.members[farthest_idx].endpoint_index;
        let points = if ep_idx == 0 {
            sequence
        } else {
            let mut rev = sequence;
            rev.reverse();
            rev
        };

        let saved = face.members[farthest_idx].route.clone();
        let before = crossings_involving(route_by_id, &farthest_id);
        let before_shared = shared_segment_count_involving(route_by_id, std::slice::from_ref(&farthest_id));
        let candidate = route_with_points(&saved, points, None);
        route_by_id.insert(farthest_id.clone(), candidate.clone());

        let farthest_rel = rel_by_id.get(&farthest_id).map(|r| Relationship { from: &r.from, to: &r.to });
        let worse = crossings_involving(route_by_id, &farthest_id) >= before
            || farthest_rel.as_ref().map(|r| route_collides_with_non_endpoints(&candidate, r, &route_input)).unwrap_or(false)
            || shared_segment_count_involving(route_by_id, std::slice::from_ref(&farthest_id)) > before_shared;
        if worse {
            route_by_id.insert(farthest_id, saved);
        }
    }
}

// ---------------------------------------------------------------------------
// Private: oppositeEndpointProjection for MountInput
// ---------------------------------------------------------------------------

/// Port of JS `oppositeEndpointProjection(endpoint, routeById, input)` using MountInput.
fn opposite_endpoint_projection_mount(
    relationship: &MountRelationship,
    endpoint_index: usize,
    side: &str,
    input: &MountInput<'_>,
) -> f64 {
    let opposite_node_id = if endpoint_index == 0 {
        &relationship.to
    } else {
        &relationship.from
    };
    if let Some(mr) = input.node_rects.get(opposite_node_id) {
        let center = rect_center(&mr.rect);
        if side == "top" || side == "bottom" { center.x } else { center.y }
    } else {
        0.0
    }
}

// Expose `SurfaceEndpointDesc` re-use — it is defined in optimize.rs as pub(super).
// We need to clone it in spread_shared_side_endpoints above. Since it is pub(super)
// and we are in the same super (route_mount_model), it is accessible.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Rect};
    use indexmap::IndexMap;

    fn mk_rect(x: f64, y: f64, w: f64, h: f64) -> MountRect {
        MountRect { rect: Rect { x, y, width: w, height: h }, fixed_ports: false, side_anchors: None }
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

    fn mk_route(points: Vec<(f64, f64)>) -> RouteData {
        let pts: Vec<Point> = points.iter().map(|&(x, y)| Point { x, y }).collect();
        RouteData {
            d: String::new(),
            points: pts,
            controls: None,
            samples: vec![],
            sample_bounds: Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
            bends: 0,
            label_x: 0.0,
            label_y: 0.0,
            style: "orthogonal".into(),
            extra: IndexMap::new(),
        }
    }

    fn mk_input<'a>(
        visible: &'a [String],
        node_rects: &'a IndexMap<String, MountRect>,
    ) -> MountInput<'a> {
        let lane_idx: IndexMap<String, i64> = IndexMap::new();
        let row_idx: IndexMap<String, i64> = IndexMap::new();
        MountInput {
            visible_node_ids: visible,
            node_rects,
            lane_index_by_node: Box::leak(Box::new(lane_idx)),
            row_index_by_node: Box::leak(Box::new(row_idx)),
            canvas_width: 1000.0,
            canvas_height: 1000.0,
        }
    }

    // -----------------------------------------------------------------------
    // reduceCrossingsBySurfaceSwaps — no-op when < 2 routes
    // -----------------------------------------------------------------------

    #[test]
    fn reduce_crossings_noop_single_route() {
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        rbd.insert("r1".to_string(), mk_route(vec![(0.0, 50.0), (100.0, 50.0)]));
        let rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        let node_rects: IndexMap<String, MountRect> = IndexMap::new();
        let visible: Vec<String> = vec![];
        let input = mk_input(&visible, &node_rects);
        reduce_crossings_by_surface_swaps(&mut rbd, &rel_by_id, &input);
        assert_eq!(rbd["r1"].points[0], Point { x: 0.0, y: 50.0 });
    }

    #[test]
    fn reduce_crossings_noop_too_many_routes() {
        // > 80 routes → no-op.
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        for i in 0..=80 {
            rbd.insert(format!("r{i}"), mk_route(vec![(0.0, i as f64), (100.0, i as f64)]));
        }
        let rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        let node_rects: IndexMap<String, MountRect> = IndexMap::new();
        let visible: Vec<String> = vec![];
        let input = mk_input(&visible, &node_rects);
        // Just confirm it doesn't panic and returns quickly.
        reduce_crossings_by_surface_swaps(&mut rbd, &rel_by_id, &input);
    }

    // -----------------------------------------------------------------------
    // routeReciprocalPairsParallel — basic smoke
    // -----------------------------------------------------------------------

    #[test]
    fn route_reciprocal_pairs_parallel_no_pairs_noop() {
        // Two non-reciprocal routes → no change.
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        rbd.insert("r1".to_string(), mk_route(vec![(0.0, 50.0), (100.0, 50.0)]));
        rbd.insert("r2".to_string(), mk_route(vec![(0.0, 100.0), (100.0, 100.0)]));
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("r1".to_string(), mk_rel("r1", "A", "B"));
        rel_by_id.insert("r2".to_string(), mk_rel("r2", "C", "D"));
        let node_rects: IndexMap<String, MountRect> = IndexMap::new();
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string(), "D".to_string()];
        let input = mk_input(&visible, &node_rects);
        route_reciprocal_pairs_parallel(&mut rbd, &rel_by_id, &input, None);
        // Routes unchanged (no reciprocal pair found).
        assert_eq!(rbd["r1"].points[0].y, 50.0);
        assert_eq!(rbd["r2"].points[0].y, 100.0);
    }

    #[test]
    fn route_reciprocal_pairs_parallel_basic_pair() {
        // Node: A→B (request, displayIndex=0) horizontal at y=50; B→A (return) should be
        // offset to y = 50+12 = 62 or y = 50-12 = 38 (whichever avoids collision).
        // No nodes in the way → first offset (PARALLEL_OFFSET=12) wins → y=50-12=38?
        // Wait: request reversed = [(100,50),(0,50)]. offsetOrthogonalPolyline(reversed, 12)
        // For horizontal rightward (dirX=-1, dirY=0) → normalX=0, normalY=1 → each pt shifts y+12.
        // reversed pts: (100,50)→(0,50) is leftward (dirX=-1,dirY=0) → normalX=dirY=0, normalY=-dirX=1
        // So offset=12 → y+12*1 = y=62. First candidate y=62.
        // No collision → keeps y=62.
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        rbd.insert("req".to_string(), mk_route(vec![(0.0, 50.0), (100.0, 50.0)]));
        rbd.insert("ret".to_string(), mk_route(vec![(100.0, 50.0), (0.0, 50.0)]));
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("req".to_string(), mk_rel("req", "A", "B"));
        rel_by_id.insert("ret".to_string(), mk_rel("ret", "B", "A"));
        let node_rects: IndexMap<String, MountRect> = IndexMap::new();
        let visible = vec!["A".to_string(), "B".to_string()];
        let input = mk_input(&visible, &node_rects);
        route_reciprocal_pairs_parallel(&mut rbd, &rel_by_id, &input, None);
        // The return should be shifted to y=62 (parallel offset of +12 from reversed request).
        let ret_y = rbd["ret"].points[0].y;
        assert!((ret_y - 62.0).abs() < 0.01, "expected y=62, got {ret_y}");
    }

    // -----------------------------------------------------------------------
    // reorderSharedSurfaceMounts — single-member group is no-op
    // -----------------------------------------------------------------------

    #[test]
    fn reorder_shared_surface_mounts_single_member_noop() {
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        // Single route on A.right face → only one member, no reorder.
        rbd.insert("r1".to_string(), mk_route(vec![(100.0, 50.0), (200.0, 50.0)]));
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("r1".to_string(), mk_rel("r1", "A", "B"));
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        // A: right edge at x=100
        node_rects.insert("A".to_string(), mk_rect(50.0, 25.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), mk_rect(200.0, 25.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string()];
        let input = mk_input(&visible, &node_rects);
        reorder_shared_surface_mounts(&mut rbd, &rel_by_id, &input);
        assert_eq!(rbd["r1"].points[0].y, 50.0); // unchanged
    }

    // -----------------------------------------------------------------------
    // recenterSingletonSideEndpoints — empty input → empty output
    // -----------------------------------------------------------------------

    #[test]
    fn recenter_singleton_empty_input() {
        let result = recenter_singleton_side_endpoints(
            &[],
            &IndexMap::new(),
            &mk_input(&[], &IndexMap::new()),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn recenter_singleton_preserves_non_singleton() {
        // Two routes on same face → count = 2 → not singleton → routes unchanged.
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        // A: left face at x=0, y=0..100
        node_rects.insert("A".to_string(), mk_rect(0.0, 0.0, 50.0, 100.0));
        node_rects.insert("B".to_string(), mk_rect(200.0, 0.0, 50.0, 100.0));
        node_rects.insert("C".to_string(), mk_rect(200.0, 150.0, 50.0, 100.0));
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("r1".to_string(), mk_rel("r1", "A", "B"));
        rel_by_id.insert("r2".to_string(), mk_rel("r2", "A", "C"));
        let planned: Vec<(String, RouteData)> = vec![
            // Both start on A.right face (x=50)
            ("r1".to_string(), mk_route(vec![(50.0, 30.0), (200.0, 30.0)])),
            ("r2".to_string(), mk_route(vec![(50.0, 70.0), (200.0, 200.0)])),
        ];
        let input = mk_input(&visible, &node_rects);
        let result = recenter_singleton_side_endpoints(&planned, &rel_by_id, &input);
        // r1 starts on A.right (x=50, y in 0..100 → "right"). count=2 → not singleton → y unchanged.
        assert_eq!(result[0].1.points[0].y, 30.0);
        assert_eq!(result[1].1.points[0].y, 70.0);
    }

    // -----------------------------------------------------------------------
    // orderGutterLanesByTarget — no-op when < 3 members per face
    // -----------------------------------------------------------------------

    #[test]
    fn order_gutter_lanes_fewer_than_3_noop() {
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        rbd.insert("r1".to_string(), mk_route(vec![(50.0, 50.0), (100.0, 50.0)]));
        rbd.insert("r2".to_string(), mk_route(vec![(50.0, 70.0), (200.0, 70.0)]));
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("r1".to_string(), mk_rel("r1", "A", "B"));
        rel_by_id.insert("r2".to_string(), mk_rel("r2", "A", "C"));
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        // A.right at x=50
        node_rects.insert("A".to_string(), mk_rect(0.0, 0.0, 50.0, 100.0));
        node_rects.insert("B".to_string(), mk_rect(100.0, 40.0, 50.0, 20.0));
        node_rects.insert("C".to_string(), mk_rect(200.0, 60.0, 50.0, 20.0));
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let input = mk_input(&visible, &node_rects);
        let r1_before = rbd["r1"].points.clone();
        let r2_before = rbd["r2"].points.clone();
        order_gutter_lanes_by_target(&mut rbd, &rel_by_id, &input);
        // < 3 members on A.right → no change.
        assert_eq!(rbd["r1"].points, r1_before);
        assert_eq!(rbd["r2"].points, r2_before);
    }

    // -----------------------------------------------------------------------
    // crossingsTouching (private) — via reduce_crossings
    // -----------------------------------------------------------------------

    #[test]
    fn crossings_touching_includes_both_ids() {
        // H×V cross — both in ids → counted once.
        let mut rbd: IndexMap<String, RouteData> = IndexMap::new();
        rbd.insert("h".to_string(), mk_route(vec![(0.0, 50.0), (100.0, 50.0)]));
        rbd.insert("v".to_string(), mk_route(vec![(50.0, 0.0), (50.0, 100.0)]));
        let ids = vec!["h".to_string(), "v".to_string()];
        assert_eq!(crossings_touching(&rbd, &ids), 1);
    }
}
