//! Private utility functions shared across all mount-model submodules.
//!
//! All items are `pub(super)` so siblings can import them; nothing here is
//! part of the external API.

use indexmap::IndexMap;

use crate::js_compat::js_hypot;
use crate::model::{Point, Rect};
use crate::route_constants::{rect_center, MOUNT_COST};
use crate::route_edges::{RouteData, RouteInput, Relationship};
use crate::route_intent::IntentRelationship;

use super::types::{MountCostFactors, MountInput, MountRect, MountRelationship};

// ---------------------------------------------------------------------------
// pointKey
// ---------------------------------------------------------------------------

pub(super) fn point_key(p: &Point) -> String {
    format!("{},{}", p.x, p.y)
}

// ---------------------------------------------------------------------------
// SIDES / SIDE_NORMAL / side_normal
// ---------------------------------------------------------------------------

pub(super) const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

pub(super) const SIDE_NORMAL: [(&str, Point); 4] = [
    ("top",    Point { x: 0.0, y: -1.0 }),
    ("bottom", Point { x: 0.0, y:  1.0 }),
    ("left",   Point { x: -1.0, y: 0.0 }),
    ("right",  Point { x:  1.0, y: 0.0 }),
];

pub(super) fn side_normal(side: &str) -> Option<&'static Point> {
    SIDE_NORMAL.iter().find(|(s, _)| *s == side).map(|(_, n)| n)
}

// ---------------------------------------------------------------------------
// isStraightFacing (private to module)
// ---------------------------------------------------------------------------

/// JS `isStraightFacing(route)`
pub(super) fn is_straight_facing(route: &RouteData) -> bool {
    if route.points.len() != 2 {
        return false;
    }
    let a = &route.points[0];
    let b = &route.points[1];
    a.x == b.x || a.y == b.y
}

// ---------------------------------------------------------------------------
// snapshot / restore routes
// ---------------------------------------------------------------------------

/// Deterministic deep clone of routeById for trial/accept.
pub(super) fn snapshot_routes(route_by_id: &IndexMap<String, RouteData>) -> IndexMap<String, RouteData> {
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
pub(super) fn restore_routes(route_by_id: &mut IndexMap<String, RouteData>, saved: &IndexMap<String, RouteData>) {
    for (id, route) in saved {
        route_by_id.insert(id.clone(), route.clone());
    }
}

// ---------------------------------------------------------------------------
// extract_rects / make_route_input / make_relationship / make_intent_relationship
// ---------------------------------------------------------------------------

// extract_rects creates a temporary IndexMap<String, Rect> for RouteInput use.
pub(super) fn extract_rects(node_rects: &IndexMap<String, MountRect>) -> IndexMap<String, Rect> {
    node_rects
        .iter()
        .map(|(k, v)| (k.clone(), v.rect.clone()))
        .collect()
}

pub(super) fn make_route_input<'a>(
    visible_node_ids: &'a [String],
    rects: &'a IndexMap<String, Rect>,
) -> RouteInput<'a> {
    RouteInput {
        visible_node_ids,
        node_rects: rects,
    }
}

pub(super) fn make_relationship<'a>(rel: &'a MountRelationship) -> Relationship<'a> {
    Relationship {
        from: &rel.from,
        to: &rel.to,
    }
}

pub(super) fn make_intent_relationship(rel: &MountRelationship) -> IntentRelationship {
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
// routeLength / nodeGapLength
// ---------------------------------------------------------------------------

/// Total wire length (Manhattan for orthogonal routes). Not exported (private helper).
pub(super) fn route_length(route: &RouteData) -> f64 {
    let mut total = 0.0f64;
    for i in 0..route.points.len().saturating_sub(1) {
        let dx = route.points[i + 1].x - route.points[i].x;
        let dy = route.points[i + 1].y - route.points[i].y;
        total += js_hypot(dx, dy);
    }
    total
}

/// Shortest possible wire between two nodes: the Manhattan gap between bounding
/// boxes (0 on axes where they overlap).
pub(super) fn node_gap_length(from_rect: Option<&Rect>, to_rect: Option<&Rect>) -> f64 {
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
// weightedMountCost
// ---------------------------------------------------------------------------

pub(super) fn weighted_mount_cost(factors: &MountCostFactors) -> f64 {
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
// side_faces_partner / ideal_facing_side (used by relief.rs)
// ---------------------------------------------------------------------------

pub(super) fn side_faces_partner(side: &str, rect: &Rect, partner_rect: &Rect) -> bool {
    let center = rect_center(rect);
    let partner = rect_center(partner_rect);
    let normal = match side_normal(side) {
        Some(n) => n,
        None => return false,
    };
    normal.x * (partner.x - center.x) + normal.y * (partner.y - center.y) > 0.0
}

pub(super) fn ideal_facing_side(rect: &Rect, partner_rect: &Rect) -> &'static str {
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
// noHardFactorWorsening / facingPolishCost (used by optimize.rs)
// ---------------------------------------------------------------------------

pub(super) fn no_hard_factor_worsening(before: &MountCostFactors, after: &MountCostFactors) -> bool {
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

pub(super) fn facing_polish_cost(non_facing: usize, factors: &MountCostFactors) -> f64 {
    (non_facing as f64) * MOUNT_COST.intent_mismatch
        + factors.bend * MOUNT_COST.bend
        + factors.length * MOUNT_COST.length
}

// ---------------------------------------------------------------------------
// SurfaceEndpoint + surface_endpoint_groups + respread_surfaces
// (needed by relief.rs and optimize.rs)
// ---------------------------------------------------------------------------

pub(super) struct SurfaceEndpoint {
    pub id: String,
    pub endpoint_index: usize,
    pub rect: Rect,
    pub side: String,
    pub opp_centre: f64,
    pub display_index: i64,
}

pub(super) fn surface_endpoint_groups(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) -> IndexMap<String, Vec<SurfaceEndpoint>> {
    use crate::route_edges::{endpoint_side, side_needs_post_selection_centering};
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

pub(super) fn respread_surfaces(
    route_by_id: &mut IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
    node_rects: &IndexMap<String, MountRect>,
) {
    use crate::js_compat::js_locale_compare;
    use crate::route_edges::{endpoint_spread_offset, offset_endpoint_route};

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
// MountInput shorthands needed across siblings
// ---------------------------------------------------------------------------

/// Build a MountInput-compatible RouteInput for collision/traversal checks.
#[allow(dead_code)] // was unused in the original monolithic file; retained for completeness
pub(super) fn make_route_input_from_mount<'a>(
    input: &MountInput<'a>,
    rects: &'a IndexMap<String, Rect>,
) -> RouteInput<'a> {
    RouteInput {
        visible_node_ids: input.visible_node_ids,
        node_rects: rects,
    }
}

// ---------------------------------------------------------------------------
// reciprocalPairsInner (private — used by relief.rs and distribution.rs)
// ---------------------------------------------------------------------------

/// Finds all unordered reciprocal pairs (routes between the same node pair
/// in opposite directions). Returns each pair sorted by JS locale order.
pub(super) fn reciprocal_pairs_inner(
    route_by_id: &IndexMap<String, RouteData>,
    rel_by_id: &IndexMap<String, MountRelationship>,
) -> Vec<[String; 2]> {
    use crate::js_compat::js_locale_compare;
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
