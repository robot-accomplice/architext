//! Distribution passes: spread endpoint positions evenly across node faces.
//! shared_segment_count_involving, crossing_count_involving,
//! keep_mount_moves_unless_worse, distribute_facing_reciprocal_surfaces,
//! distribute_surface_mount_units, straighten_self_crossing_pairs,
//! mirror_self_crossing_bundles.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::js_compat::{js_default_sort_cmp, js_locale_compare};
use crate::model::Point;
use crate::route_constants::RECIPROCAL_PARALLEL_OFFSET;
use crate::route_edges::{
    axis_aligned_segments, endpoint_side, offset_endpoint_route,
    route_collides_with_non_endpoints, route_with_points, shared_segment_length,
    side_endpoint_key, side_needs_post_selection_centering, spread_unit_slots, Relationship,
    RouteData,
};
use crate::route_reciprocal::{reciprocal_pairs_by_adjacency, Relationship as ReciprocalRelationship};

use super::cost::route_intersections;
use super::helpers::{extract_rects, make_route_input};
use super::optimize::{opposite_route_endpoint_projection, SurfaceEndpointDesc};
use super::types::{MountInput, MountRect, MountRelationship};

// ---------------------------------------------------------------------------
// sharedSegmentCountInvolving
// ---------------------------------------------------------------------------

/// Port of JS `sharedSegmentCountInvolving(routeById, ids)`.
///
/// Counts visible segment overlaps that involve one of `ids` (each unordered
/// pair counted once).
pub fn shared_segment_count_involving(
    route_by_id: &IndexMap<String, RouteData>,
    ids: &[String],
) -> usize {
    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let mut total = 0usize;
    for id in ids {
        let route = match route_by_id.get(id) { Some(r) => r, None => continue };
        let segments = axis_aligned_segments(route);
        for (other_id, other_route) in route_by_id {
            if other_id == id { continue; }
            // Dedup: for two affected ids, only count the pair with the lower id.
            if id_set.contains(other_id.as_str()) && other_id.as_str() < id.as_str() { continue; }
            let other_segments = axis_aligned_segments(other_route);
            for seg in &segments {
                for other_seg in &other_segments {
                    if shared_segment_length(seg, other_seg) > 1.0 {
                        total += 1;
                    }
                }
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// crossingCountInvolving
// ---------------------------------------------------------------------------

/// Port of JS `crossingCountInvolving(routeById, ids)`.
///
/// Total rendered crossings touching any of `ids`, counted against EVERY
/// route. Mirrors `shared_segment_count_involving`'s dedup so a crossing
/// between two affected edges is counted once.
pub fn crossing_count_involving(
    route_by_id: &IndexMap<String, RouteData>,
    ids: &[String],
) -> usize {
    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let mut total = 0usize;
    for id in ids {
        let route = match route_by_id.get(id) { Some(r) => r, None => continue };
        for (other_id, other_route) in route_by_id {
            if other_id == id { continue; }
            if id_set.contains(other_id.as_str()) && other_id.as_str() < id.as_str() { continue; }
            total += route_intersections(route, other_route);
        }
    }
    total
}

// ---------------------------------------------------------------------------
// keepMountMovesUnlessWorse
// ---------------------------------------------------------------------------

/// Port of JS `keepMountMovesUnlessWorse(routeById, relationshipById, input, ids, applyMoves)`.
///
/// Shared guard for distribution passes: applies `apply_moves` (mutates
/// `route_by_id`), then keeps the result only if it added no bend, no node
/// collision, no shared visible segment, and no crossing; otherwise restores
/// saved routes. Returns `true` when the moves were kept.
pub fn keep_mount_moves_unless_worse(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
    ids: &[String],
    apply_moves: impl FnOnce(&mut IndexMap<String, RouteData>),
) -> bool {
    let saved: Vec<(String, RouteData)> = ids.iter()
        .filter_map(|id| route_by_id.get(id).map(|r| (id.clone(), r.clone())))
        .collect();
    let before_bends: usize = saved.iter().map(|(_, r)| r.bends).sum();
    let before_shared = shared_segment_count_involving(route_by_id, ids);
    let before_crossings = crossing_count_involving(route_by_id, ids);

    apply_moves(route_by_id);

    let after_bends: usize = ids.iter()
        .filter_map(|id| route_by_id.get(id))
        .map(|r| r.bends)
        .sum();
    let rects_plain = extract_rects(input.node_rects);
    let route_input = make_route_input(input.visible_node_ids, &rects_plain);
    let collides = ids.iter().any(|id| {
        let route = match route_by_id.get(id) { Some(r) => r, None => return false };
        let rel = match rel_by_id.get(id) { Some(r) => r, None => return false };
        let simple_rel = Relationship { from: &rel.from, to: &rel.to };
        route_collides_with_non_endpoints(route, &simple_rel, &route_input)
    });

    if collides
        || after_bends > before_bends
        || shared_segment_count_involving(route_by_id, ids) > before_shared
        || crossing_count_involving(route_by_id, ids) > before_crossings
    {
        for (id, route) in &saved { route_by_id.insert(id.clone(), route.clone()); }
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// distributeFacingReciprocalSurfaces
// ---------------------------------------------------------------------------

/// Port of JS `distributeFacingReciprocalSurfaces(routeById, relationshipById, input)`.
///
/// Even out STRAIGHT FACING runs between two adjacent nodes. When a node pair
/// exchanges several straight reciprocal runs across one facing surface-pair
/// both ends move together to keep the run straight while the set spreads.
pub fn distribute_facing_reciprocal_surfaces(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    // Count flow endpoints per face for the "dedicated faces only" guard.
    let mut face_occupancy: IndexMap<String, usize> = IndexMap::new();
    for (relationship_id, route) in route_by_id.iter() {
        let relationship = match rel_by_id.get(relationship_id) { Some(r) => r, None => continue };
        if relationship.relationship_type != "flow" || route.points.is_empty() { continue; }
        let pairs: [(&str, &Point); 2] = [
            (&relationship.from, &route.points[0]),
            (&relationship.to, route.points.last().unwrap()),
        ];
        for (node_id, point) in &pairs {
            let mount_rect = match input.node_rects.get(*node_id) { Some(r) => r, None => continue };
            let side = endpoint_side(&mount_rect.rect, point);
            if side.is_empty() { continue; }
            let face_key = side_endpoint_key(node_id, side);
            *face_occupancy.entry(face_key).or_insert(0) += 1;
        }
    }

    struct RunDesc {
        relationship_id: String,
        current: f64,
        opposite_center: f64,
        display_index: i64,
    }

    struct GroupDesc {
        axis: &'static str,
        lo: f64,
        hi: f64,
        runs: Vec<RunDesc>,
    }

    let mut groups: IndexMap<String, GroupDesc> = IndexMap::new();

    for (relationship_id, route) in route_by_id.iter() {
        let relationship = match rel_by_id.get(relationship_id) { Some(r) => r, None => continue };
        if relationship.relationship_type != "flow" { continue; }
        if route.points.is_empty() || route.points.len() != 2 { continue; } // only clean straight runs
        let from_mr = match input.node_rects.get(&relationship.from) { Some(r) => r, None => continue };
        let to_mr = match input.node_rects.get(&relationship.to) { Some(r) => r, None => continue };
        if from_mr.fixed_ports || to_mr.fixed_ports { continue; }
        let start = &route.points[0];
        let end = &route.points[1];
        let from_side = endpoint_side(&from_mr.rect, start);
        let to_side = endpoint_side(&to_mr.rect, end);
        if !side_needs_post_selection_centering(from_side) || !side_needs_post_selection_centering(to_side) { continue; }
        let horizontal = start.y == end.y
            && (from_side == "left" || from_side == "right")
            && (to_side == "left" || to_side == "right");
        let vertical = start.x == end.x
            && (from_side == "top" || from_side == "bottom")
            && (to_side == "top" || to_side == "bottom");
        if !horizontal && !vertical { continue; }
        let axis: &'static str = if horizontal { "y" } else { "x" };

        let mut key_parts = [
            side_endpoint_key(&relationship.from, from_side),
            side_endpoint_key(&relationship.to, to_side),
        ];
        key_parts.sort();
        let key = format!("{}|{}", key_parts[0], key_parts[1]);

        if !groups.contains_key(&key) {
            let (from_start, from_len, to_start, to_len) = if axis == "y" {
                (from_mr.rect.y, from_mr.rect.height, to_mr.rect.y, to_mr.rect.height)
            } else {
                (from_mr.rect.x, from_mr.rect.width, to_mr.rect.x, to_mr.rect.width)
            };
            let lo = f64::max(from_start, to_start);
            let hi = f64::min(from_start + from_len, to_start + to_len);
            groups.insert(key.clone(), GroupDesc { axis, lo, hi, runs: Vec::new() });
        }
        let group = groups.get_mut(key.as_str()).unwrap();
        if group.axis != axis { continue; }
        let opposite_center = if axis == "y" {
            to_mr.rect.y + to_mr.rect.height / 2.0
        } else {
            to_mr.rect.x + to_mr.rect.width / 2.0
        };
        let current = if axis == "y" { start.y } else { start.x };
        group.runs.push(RunDesc {
            relationship_id: relationship_id.clone(),
            current,
            opposite_center,
            display_index: relationship.display_index,
        });
    }

    // Collect group keys to iterate (avoid borrow of groups while mutating route_by_id).
    let group_keys: Vec<String> = groups.keys().cloned().collect();
    for key in &group_keys {
        let group = groups.get_mut(key.as_str()).unwrap();
        if group.runs.len() < 2 || group.hi <= group.lo { continue; }
        // Only re-space when both facing surfaces carry nothing but this group.
        let key_parts: Vec<&str> = key.splitn(2, '|').collect();
        let dedicated = key_parts.iter().all(|fk| {
            face_occupancy.get(*fk).copied().unwrap_or(0) == group.runs.len()
        });
        if !dedicated { continue; }

        group.runs.sort_by(|left, right| {
            left.opposite_center.partial_cmp(&right.opposite_center).unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.display_index.cmp(&right.display_index))
                .then_with(|| js_locale_compare(&left.relationship_id, &right.relationship_id))
        });

        let lo = group.lo;
        let hi = group.hi;
        let n = group.runs.len();
        let targets: Vec<f64> = (0..n).map(|i| lo + ((i + 1) as f64 / (n + 1) as f64) * (hi - lo)).collect();

        // Skip if already at target (avoid spurious collinear waypoints).
        let already_even = group.runs.iter().enumerate().all(|(i, run)| (run.current - targets[i]).abs() < 0.5);
        if already_even { continue; }

        let axis = group.axis;
        let ids: Vec<String> = group.runs.iter().map(|r| r.relationship_id.clone()).collect();
        let run_ids: Vec<String> = ids.clone();
        let targets_copy = targets.clone();
        keep_mount_moves_unless_worse(route_by_id, rel_by_id, input, &ids, |rbid| {
            for (i, rel_id) in run_ids.iter().enumerate() {
                let route = match rbid.get(rel_id) { Some(r) => r.clone(), None => continue };
                let points: Vec<Point> = route.points.iter().map(|p| {
                    if axis == "y" { Point { x: p.x, y: targets_copy[i] } } else { Point { x: targets_copy[i], y: p.y } }
                }).collect();
                let controls = route.controls.clone();
                rbid.insert(rel_id.clone(), route_with_points(&route, points, controls));
            }
        });
    }
}

// ---------------------------------------------------------------------------
// distributeSurfaceMountUnits
// ---------------------------------------------------------------------------

/// Port of JS `distributeSurfaceMountUnits(routeById, relationshipById, input)`.
///
/// Even out mount DISTRIBUTION on every shared surface. Reciprocal pairs
/// count as one unit (kept parallel by translating both mounts rigidly); lone
/// edges count as one unit. Unit CENTRES are spread evenly using
/// `spread_unit_slots` (which reserves each unit's width so facing-pair
/// endpoints don't overlap). Reverts if it adds a bend, node collision, or
/// shared segment / crossing.
pub fn distribute_surface_mount_units(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    // Collect endpoints per node face.
    let mut groups: IndexMap<String, Vec<SurfaceEndpointDesc>> = IndexMap::new();
    for (relationship_id, route) in route_by_id.iter() {
        let relationship = match rel_by_id.get(relationship_id) { Some(r) => r, None => continue };
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
            groups.entry(key).or_default().push(SurfaceEndpointDesc {
                relationship: relationship.clone(),
                relationship_id: relationship_id.clone(),
                endpoint_index: *ei,
                mount_rect: mount_rect.clone(),
                side: side.to_string(),
            });
        }
    }

    let group_keys: Vec<String> = groups.keys().cloned().collect();
    for gkey in &group_keys {
        let endpoints = groups.get(gkey).unwrap();
        if endpoints.is_empty() { continue; }

        let side = &endpoints[0].side;
        let rect = &endpoints[0].mount_rect.rect;
        let axis: &'static str = if side == "left" || side == "right" { "y" } else { "x" };
        let center = if axis == "y" { rect.y + rect.height / 2.0 } else { rect.x + rect.width / 2.0 };
        let side_length = if side == "left" || side == "right" { rect.height } else { rect.width };

        // Helper: current mount coordinate for an endpoint.
        let mount_of = |ep: &SurfaceEndpointDesc, rbid: &IndexMap<String, RouteData>| -> f64 {
            let route = match rbid.get(&ep.relationship_id) { Some(r) => r, None => return 0.0 };
            let point = if ep.endpoint_index == 0 { &route.points[0] } else { route.points.last().unwrap() };
            if axis == "y" { point.y } else { point.x }
        };

        // Bundle reciprocal pairs into one unit; lone edges are their own unit.
        let mut by_node_pair: IndexMap<String, Vec<usize>> = IndexMap::new();
        for (i, ep) in endpoints.iter().enumerate() {
            let mut parts = [ep.relationship.from.as_str(), ep.relationship.to.as_str()];
            parts.sort_by(|a, b| js_default_sort_cmp(a, b));
            // JS uses space separator for distributeSurfaceMountUnits
            let pair_key = format!("{} {}", parts[0], parts[1]);
            by_node_pair.entry(pair_key).or_default().push(i);
        }

        struct UnitDesc {
            member_indices: Vec<usize>,
            opposite_center: f64,
        }
        let mut units: Vec<UnitDesc> = Vec::new();
        for member_indices in by_node_pair.values() {
            let is_reciprocal = member_indices.len() == 2 && {
                let ep0 = &endpoints[member_indices[0]];
                let ep1 = &endpoints[member_indices[1]];
                ep0.relationship.from == ep1.relationship.to
                    && ep0.relationship.to == ep1.relationship.from
            };
            if is_reciprocal {
                let ep0 = &endpoints[member_indices[0]];
                let opposite_node = if ep0.endpoint_index == 0 { &ep0.relationship.to } else { &ep0.relationship.from };
                let opp_center = input.node_rects.get(opposite_node).map(|mr| {
                    if axis == "y" { mr.rect.y + mr.rect.height / 2.0 } else { mr.rect.x + mr.rect.width / 2.0 }
                }).unwrap_or(0.0);
                units.push(UnitDesc { member_indices: member_indices.clone(), opposite_center: opp_center });
            } else {
                for &i in member_indices {
                    let ep = &endpoints[i];
                    let opposite_node = if ep.endpoint_index == 0 { &ep.relationship.to } else { &ep.relationship.from };
                    let opp_center = input.node_rects.get(opposite_node).map(|mr| {
                        if axis == "y" { mr.rect.y + mr.rect.height / 2.0 } else { mr.rect.x + mr.rect.width / 2.0 }
                    }).unwrap_or(0.0);
                    units.push(UnitDesc { member_indices: vec![i], opposite_center: opp_center });
                }
            }
        }
        if units.is_empty() { continue; }

        // Sort units: primary = opposite node centre; secondary = far landing of the unit's
        // routes; tertiary = displayIndex; quaternary = relationshipId.
        {
            let route_by_id_ref = &*route_by_id; // shared borrow for sort
            let endpoints_ref = endpoints;
            units.sort_by(|left, right| {
                left.opposite_center.partial_cmp(&right.opposite_center).unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        let l_opp: f64 = if left.member_indices.is_empty() { 0.0 } else {
                            let sum: f64 = left.member_indices.iter().map(|&i| {
                                let ep = &endpoints_ref[i];
                                opposite_route_endpoint_projection(&ep.relationship, ep.endpoint_index, &ep.side, route_by_id_ref)
                            }).sum();
                            sum / left.member_indices.len() as f64
                        };
                        let r_opp: f64 = if right.member_indices.is_empty() { 0.0 } else {
                            let sum: f64 = right.member_indices.iter().map(|&i| {
                                let ep = &endpoints_ref[i];
                                opposite_route_endpoint_projection(&ep.relationship, ep.endpoint_index, &ep.side, route_by_id_ref)
                            }).sum();
                            sum / right.member_indices.len() as f64
                        };
                        l_opp.partial_cmp(&r_opp).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| {
                        let l_di = left.member_indices.first().map(|&i| endpoints_ref[i].relationship.display_index).unwrap_or(0);
                        let r_di = right.member_indices.first().map(|&i| endpoints_ref[i].relationship.display_index).unwrap_or(0);
                        l_di.cmp(&r_di)
                    })
                    .then_with(|| {
                        let l_id = left.member_indices.first().map(|&i| endpoints_ref[i].relationship_id.as_str()).unwrap_or("");
                        let r_id = right.member_indices.first().map(|&i| endpoints_ref[i].relationship_id.as_str()).unwrap_or("");
                        js_locale_compare(l_id, r_id)
                    })
            });
        }

        // Compute unit half-widths (spread distance from unit centre to farthest member mount).
        let unit_half_widths: Vec<f64> = units.iter().map(|unit| {
            if unit.member_indices.is_empty() { return 0.0; }
            let mounts: Vec<f64> = unit.member_indices.iter().map(|&i| mount_of(&endpoints[i], route_by_id)).collect();
            let unit_center = mounts.iter().sum::<f64>() / mounts.len() as f64;
            mounts.iter().map(|&m| (m - unit_center).abs()).fold(0.0_f64, f64::max)
        }).collect();

        let slots = spread_unit_slots(&unit_half_widths, side_length);

        let affected: Vec<String> = {
            let mut seen = IndexMap::new();
            for ep in endpoints { seen.entry(ep.relationship_id.clone()).or_insert(()); }
            seen.into_keys().collect()
        };

        // Snapshot endpoints vec and unit layout for the closure.
        let endpoints_snapshot: Vec<(String, usize, MountRect, String, f64)> = endpoints.iter().map(|ep| {
            (ep.relationship_id.clone(), ep.endpoint_index, ep.mount_rect.clone(), ep.side.clone(), mount_of(ep, route_by_id))
        }).collect();
        let units_snapshot: Vec<Vec<usize>> = units.iter().map(|u| u.member_indices.clone()).collect();
        let side_owned = side.to_string();
        let rect_clone = rect.clone();

        keep_mount_moves_unless_worse(route_by_id, rel_by_id, input, &affected, |rbid| {
            for (unit_index, member_indices) in units_snapshot.iter().enumerate() {
                let target_offset = slots[unit_index];
                // Unit centre = average of member mounts (using pre-snapshot values).
                let unit_mounts: Vec<f64> = member_indices.iter().map(|&i| endpoints_snapshot[i].4).collect();
                let unit_center_val = unit_mounts.iter().sum::<f64>() / unit_mounts.len() as f64;
                // Leave alone if already at slot (< 0.5 tolerance).
                if (unit_center_val - (center + target_offset)).abs() < 0.5 { continue; }
                for (mi, &i) in member_indices.iter().enumerate() {
                    let (ref rel_id, ei, ref mr, ref s, _) = endpoints_snapshot[i];
                    let member_offset = unit_mounts[mi] - unit_center_val + target_offset;
                    let route = match rbid.get(rel_id.as_str()) { Some(r) => r.clone(), None => continue };
                    let new_route = offset_endpoint_route(&route, ei, &mr.rect, s, member_offset);
                    rbid.insert(rel_id.clone(), new_route);
                }
            }
            let _ = (&side_owned, &rect_clone); // keep alive
        });
    }
}

// ---------------------------------------------------------------------------
// straightenSelfCrossingPairs
// ---------------------------------------------------------------------------

/// Port of JS `straightenSelfCrossingPairs(routeById, relationshipById, input)`.
///
/// Final pair-aware ordering pass. When a reciprocal pair crosses itself on a
/// directly-facing surface-pair, rebuild BOTH as straight parallel runs.
/// Guarded: kept only if crossings strictly decrease, no new node collision or
/// shared segment.
pub fn straighten_self_crossing_pairs(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    // Collect flow relationships and build face occupancy map.
    let flow_rels: Vec<ReciprocalRelationship> = rel_by_id.values()
        .filter(|r| r.relationship_type == "flow")
        .map(|r| ReciprocalRelationship {
            id: r.id.clone(),
            from: r.from.clone(),
            to: r.to.clone(),
            display_index: r.display_index as f64,
        })
        .collect();

    let mut face_occupancy: IndexMap<String, usize> = IndexMap::new();
    for (relationship_id, route) in route_by_id.iter() {
        let rel = match rel_by_id.get(relationship_id) { Some(r) => r, None => continue };
        if rel.relationship_type != "flow" || route.points.is_empty() { continue; }
        let pairs: [(&str, &Point); 2] = [(&rel.from, &route.points[0]), (&rel.to, route.points.last().unwrap())];
        for (node_id, point) in &pairs {
            let mount_rect = match input.node_rects.get(*node_id) { Some(r) => r, None => continue };
            let side = endpoint_side(&mount_rect.rect, point);
            if side.is_empty() { continue; }
            let key = side_endpoint_key(node_id, side);
            *face_occupancy.entry(key).or_insert(0) += 1;
        }
    }

    // crossingsTouching: total crossings between routes touching any of `ids` (pairs counted once).
    let crossings_touching = |rbid: &IndexMap<String, RouteData>, ids: &[String]| -> usize {
        let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let entries: Vec<(&String, &RouteData)> = rbid.iter().collect();
        let mut total = 0usize;
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                if !id_set.contains(entries[i].0.as_str()) && !id_set.contains(entries[j].0.as_str()) { continue; }
                total += route_intersections(entries[i].1, entries[j].1);
            }
        }
        total
    };

    for [id_a, id_b] in reciprocal_pairs_by_adjacency(&flow_rels) {
        let route_a = match route_by_id.get(&id_a) { Some(r) => r.clone(), None => continue };
        let route_b = match route_by_id.get(&id_b) { Some(r) => r.clone(), None => continue };
        if route_intersections(&route_a, &route_b) == 0 { continue; }

        let relationship = match rel_by_id.get(&id_a) { Some(r) => r, None => continue };
        let from_mr = match input.node_rects.get(&relationship.from) { Some(r) => r, None => continue };
        let to_mr = match input.node_rects.get(&relationship.to) { Some(r) => r, None => continue };
        if from_mr.fixed_ports || to_mr.fixed_ports { continue; }

        let from_side = endpoint_side(&from_mr.rect, &route_a.points[0]);
        let to_side = endpoint_side(&to_mr.rect, route_a.points.last().unwrap());
        let vertical = (from_side == "top" || from_side == "bottom") && (to_side == "top" || to_side == "bottom");
        let horizontal = (from_side == "left" || from_side == "right") && (to_side == "left" || to_side == "right");
        if !vertical && !horizontal { continue; }

        // axis = the coordinate held constant along a straight run.
        let axis: &'static str = if vertical { "x" } else { "y" };

        let (lo, hi) = if axis == "x" {
            (f64::max(from_mr.rect.x, to_mr.rect.x),
             f64::min(from_mr.rect.x + from_mr.rect.width, to_mr.rect.x + to_mr.rect.width))
        } else {
            (f64::max(from_mr.rect.y, to_mr.rect.y),
             f64::min(from_mr.rect.y + from_mr.rect.height, to_mr.rect.y + to_mr.rect.height))
        };
        if hi - lo < RECIPROCAL_PARALLEL_OFFSET { continue; }

        // Anchor on the more-occupied face.
        let from_occ = face_occupancy.get(&side_endpoint_key(&relationship.from, from_side)).copied().unwrap_or(0);
        let to_occ   = face_occupancy.get(&side_endpoint_key(&relationship.to,   to_side)).copied().unwrap_or(0);
        let anchor_node = if from_occ >= to_occ { &relationship.from } else { &relationship.to };

        let clamp = |v: f64| -> f64 { f64::min(hi, f64::max(lo, v)) };
        let coord_on_anchor = |id: &str| -> f64 {
            let route = route_by_id.get(id).unwrap();
            let rel = rel_by_id.get(id).unwrap();
            let point = if rel.from.as_str() == anchor_node { &route.points[0] } else { route.points.last().unwrap() };
            clamp(if axis == "x" { point.x } else { point.y })
        };

        let mut coord_a = coord_on_anchor(&id_a);
        let mut coord_b = coord_on_anchor(&id_b);
        if (coord_a - coord_b).abs() < RECIPROCAL_PARALLEL_OFFSET {
            let mid = clamp((coord_a + coord_b) / 2.0);
            let half = RECIPROCAL_PARALLEL_OFFSET / 2.0;
            let a_first = coord_a <= coord_b;
            coord_a = clamp(mid + if a_first { -half } else { half });
            coord_b = clamp(mid + if a_first { half } else { -half });
        }

        let straighten = |rbid: &mut IndexMap<String, RouteData>, id: &str, coord: f64| {
            let route = rbid.get(id).unwrap().clone();
            let start = route.points[0].clone();
            let end = route.points.last().unwrap().clone();
            let points = if vertical {
                vec![Point { x: coord, y: start.y }, Point { x: coord, y: end.y }]
            } else {
                vec![Point { x: start.x, y: coord }, Point { x: end.x, y: coord }]
            };
            let controls = route.controls.clone();
            rbid.insert(id.to_string(), route_with_points(&route, points, controls));
        };

        let ids = vec![id_a.clone(), id_b.clone()];
        let saved: Vec<(String, RouteData)> = ids.iter()
            .filter_map(|id| route_by_id.get(id).map(|r| (id.clone(), r.clone())))
            .collect();
        let before_crossings = crossings_touching(route_by_id, &ids);
        let before_shared = shared_segment_count_involving(route_by_id, &ids);
        straighten(route_by_id, &id_a, coord_a);
        straighten(route_by_id, &id_b, coord_b);

        let rects_plain = extract_rects(input.node_rects);
        let route_input_plain = make_route_input(input.visible_node_ids, &rects_plain);
        let collides = ids.iter().any(|id| {
            let route = route_by_id.get(id).unwrap();
            let rel = rel_by_id.get(id).unwrap();
            let simple_rel = Relationship { from: &rel.from, to: &rel.to };
            route_collides_with_non_endpoints(route, &simple_rel, &route_input_plain)
        });
        if collides
            || crossings_touching(route_by_id, &ids) >= before_crossings
            || shared_segment_count_involving(route_by_id, &ids) > before_shared
        {
            for (id, route) in &saved { route_by_id.insert(id.clone(), route.clone()); }
        }
    }
}

// ---------------------------------------------------------------------------
// mirrorSelfCrossingBundles
// ---------------------------------------------------------------------------

/// Port of JS `mirrorSelfCrossingBundles(routeById, relationshipById, input)`.
///
/// Companion to `straighten_self_crossing_pairs` for U-bundle pairs that
/// cross themselves but cannot be straightened. Tries swapping mount order at
/// each end of the pair; keeps the first swap that strictly reduces crossings.
pub fn mirror_self_crossing_bundles(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    input: &MountInput<'_>,
) {
    let flow_rels: Vec<ReciprocalRelationship> = rel_by_id.values()
        .filter(|r| r.relationship_type == "flow")
        .map(|r| ReciprocalRelationship {
            id: r.id.clone(),
            from: r.from.clone(),
            to: r.to.clone(),
            display_index: r.display_index as f64,
        })
        .collect();

    let crossings_touching = |rbid: &IndexMap<String, RouteData>, ids: &[String]| -> usize {
        let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        let entries: Vec<(&String, &RouteData)> = rbid.iter().collect();
        let mut total = 0usize;
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                if !id_set.contains(entries[i].0.as_str()) && !id_set.contains(entries[j].0.as_str()) { continue; }
                total += route_intersections(entries[i].1, entries[j].1);
            }
        }
        total
    };

    // Swap the along-face coordinate (terminal mount + stub) of two routes at a shared node end.
    let swap_mount = |rbid: &mut IndexMap<String, RouteData>, id_a: &str, a_is_start: bool, id_b: &str, b_is_start: bool, axis: &str| {
        let ra = rbid.get(id_a).unwrap().clone();
        let rb = rbid.get(id_b).unwrap().clone();
        let mut pa: Vec<Point> = ra.points.clone();
        let mut pb: Vec<Point> = rb.points.clone();
        let a_term = if a_is_start { 0 } else { pa.len() - 1 };
        let a_stub = if a_is_start { 1 } else { pa.len().saturating_sub(2) };
        let b_term = if b_is_start { 0 } else { pb.len() - 1 };
        let b_stub = if b_is_start { 1 } else { pb.len().saturating_sub(2) };
        let (ca, cb) = if axis == "x" {
            (pa[a_term].x, pb[b_term].x)
        } else {
            (pa[a_term].y, pb[b_term].y)
        };
        if axis == "x" {
            pa[a_term].x = cb;
            if pa.len() > a_stub { pa[a_stub].x = cb; }
            pb[b_term].x = ca;
            if pb.len() > b_stub { pb[b_stub].x = ca; }
        } else {
            pa[a_term].y = cb;
            if pa.len() > a_stub { pa[a_stub].y = cb; }
            pb[b_term].y = ca;
            if pb.len() > b_stub { pb[b_stub].y = ca; }
        }
        let ra_controls = ra.controls.clone();
        let rb_controls = rb.controls.clone();
        rbid.insert(id_a.to_string(), route_with_points(&ra, pa, ra_controls));
        rbid.insert(id_b.to_string(), route_with_points(&rb, pb, rb_controls));
    };

    let vert = |side: &str| -> bool { side == "top" || side == "bottom" };
    let horiz = |side: &str| -> bool { side == "left" || side == "right" };

    let rects_plain = extract_rects(input.node_rects);
    let route_input_plain = make_route_input(input.visible_node_ids, &rects_plain);

    for [id_a, id_b] in reciprocal_pairs_by_adjacency(&flow_rels) {
        let route_a = match route_by_id.get(&id_a) { Some(r) => r.clone(), None => continue };
        let route_b = match route_by_id.get(&id_b) { Some(r) => r.clone(), None => continue };
        if route_intersections(&route_a, &route_b) == 0 { continue; }

        let relationship = match rel_by_id.get(&id_a) { Some(r) => r, None => continue };
        let from_mr = match input.node_rects.get(&relationship.from) { Some(r) => r, None => continue };
        let to_mr = match input.node_rects.get(&relationship.to) { Some(r) => r, None => continue };
        if from_mr.fixed_ports || to_mr.fixed_ports { continue; }

        let from_side = endpoint_side(&from_mr.rect, &route_a.points[0]);
        let to_side = endpoint_side(&to_mr.rect, route_a.points.last().unwrap());
        if !((vert(from_side) && vert(to_side)) || (horiz(from_side) && horiz(to_side))) { continue; }
        let axis = if vert(from_side) { "x" } else { "y" };

        // Try from-node swap (a_is_start=true, b_is_start=false) then to-node swap.
        for (a_is_start, b_is_start) in [(true, false), (false, true)] {
            let ids = vec![id_a.clone(), id_b.clone()];
            let saved: Vec<(String, RouteData)> = ids.iter()
                .filter_map(|id| route_by_id.get(id).map(|r| (id.clone(), r.clone())))
                .collect();
            let before_crossings = crossings_touching(route_by_id, &ids);
            let before_shared = shared_segment_count_involving(route_by_id, &ids);
            swap_mount(route_by_id, &id_a, a_is_start, &id_b, b_is_start, axis);
            let collides = ids.iter().any(|id| {
                let route = route_by_id.get(id).unwrap();
                let rel = rel_by_id.get(id).unwrap();
                let simple_rel = Relationship { from: &rel.from, to: &rel.to };
                route_collides_with_non_endpoints(route, &simple_rel, &route_input_plain)
            });
            if collides
                || crossings_touching(route_by_id, &ids) >= before_crossings
                || shared_segment_count_involving(route_by_id, &ids) > before_shared
            {
                for (id, route) in &saved { route_by_id.insert(id.clone(), route.clone()); }
                continue;
            }
            break; // kept an improving swap
        }
    }
}
