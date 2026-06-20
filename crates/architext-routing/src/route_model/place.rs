//! Component 1 — diagram placement loop.
//!
//! Runs [`best_clean_route`] across an edge set: each edge sees the node field as
//! obstacles (excluding its own endpoints) and the already-placed routes (for
//! crossings). Edges that find no clean 0/1-bend route are left for Component 2
//! and reported as the fallback set — the feasibility split that sizes the rest
//! of the work (ROUTING_DETERMINISTIC_MODEL.md §5 step 1, "feasibility split").
//!
//! NOTE: mounts are side-centres here, so parallel edges sharing a surface still
//! overlap — mount-slot spreading + the eviction cascade are later slices. This
//! loop's purpose now is the clean/fallback COUNT and the clean edges' β.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::component2::monotone_detour;
use super::select::{best_clean_route, polyline_crossings, side_center_mounts, Candidate};
use super::{bend_score, build_path_01, clears, Side};
use crate::model::{Point, Rect};
use crate::route_geometry::route_length;

/// A directed edge as indices into the node-rect list.
#[derive(Debug, Clone, Copy)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
}

/// Result of placing an edge set with Component 1.
#[derive(Debug, Clone)]
pub struct Placement {
    /// Per-edge clean route (None ⇒ fell through to Component 2).
    pub routes: Vec<Option<Vec<Point>>>,
    /// Per-edge chosen candidate (sides), when clean.
    pub candidates: Vec<Option<Candidate>>,
    pub clean_count: usize,
    pub fallback_count: usize,
}

/// Place every edge with Component 1, in the given (deterministic) order.
/// Obstacles for an edge = all node rects except its two endpoints. Each placed
/// clean route is added to the field so later edges score crossings against it.
///
/// Processing order is the caller's order for now; most-constrained-first is a
/// later refinement (it only changes the greedy crossing outcome, not clean/
/// fallback feasibility, which is per-edge geometry).
pub fn place_clean(nodes: &[Rect], edges: &[Edge]) -> Placement {
    let mut placed: Vec<Vec<Point>> = Vec::new();
    let mut routes: Vec<Option<Vec<Point>>> = Vec::with_capacity(edges.len());
    let mut candidates: Vec<Option<Candidate>> = Vec::with_capacity(edges.len());

    for e in edges {
        let a = &nodes[e.a];
        let b = &nodes[e.b];
        let obstacles: Vec<Rect> = nodes
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != e.a && *j != e.b)
            .map(|(_, r)| r.clone())
            .collect();

        match best_clean_route(a, b, &obstacles, &placed) {
            Some(c) => {
                placed.push(c.points.clone());
                routes.push(Some(c.points.clone()));
                candidates.push(Some(c));
            }
            None => {
                routes.push(None);
                candidates.push(None);
            }
        }
    }

    let clean_count = routes.iter().filter(|r| r.is_some()).count();
    Placement {
        fallback_count: edges.len() - clean_count,
        clean_count,
        routes,
        candidates,
    }
}

/// Lexicographic `(β, crossings, length)` compare for forced-detour selection.
fn lex(a: &(f64, usize, f64), b: &(f64, usize, f64)) -> Ordering {
    a.0.partial_cmp(&b.0)
        .unwrap_or(Ordering::Equal)
        .then(a.1.cmp(&b.1))
        .then(a.2.partial_cmp(&b.2).unwrap_or(Ordering::Equal))
}

/// Forced-case route (Component 2): no clean 0/1-bend exists, so pick the
/// surface-mount pair whose monotone detour is cheapest by `(β, crossings,
/// length)`. Every candidate is monotone, so the result is never a dogleg.
fn forced_detour(a: &Rect, b: &Rect, obstacles: &[Rect], placed: &[Vec<Point>]) -> Option<Vec<Point>> {
    let mut best: Option<((f64, usize, f64), Vec<Point>)> = None;
    for (_, pa) in side_center_mounts(a) {
        for (_, pb) in side_center_mounts(b) {
            if let Some(path) = monotone_detour(&pa, &pb, obstacles) {
                let beta = bend_score(&path);
                let crossings = placed.iter().map(|pl| polyline_crossings(&path, pl)).sum();
                let length = route_length(&path);
                let cost = (beta, crossings, length);
                if best.as_ref().map(|(c, _)| lex(&cost, c) == Ordering::Less).unwrap_or(true) {
                    best = Some((cost, path));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// A routed edge plus the surfaces it mounts (`Some` ⇒ a clean Component-1 route;
/// `None` ⇒ a Component-2 detour, whose mounts are chosen internally).
#[derive(Debug, Clone)]
pub struct RoutedEdge {
    pub route: Vec<Point>,
    pub sides: Option<(Side, Side)>,
}

/// Obstacles for an edge = all node rects except its two endpoints.
fn obstacles_for(nodes: &[Rect], a: usize, b: usize) -> Vec<Rect> {
    nodes
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != a && *j != b)
        .map(|(_, r)| r.clone())
        .collect()
}

/// Route every edge, capturing the chosen surfaces. Component 1 (clean) when
/// possible, else Component 2 (forced monotone detour). Always monotone.
pub fn route_all_sided(nodes: &[Rect], edges: &[Edge]) -> Vec<RoutedEdge> {
    let mut placed: Vec<Vec<Point>> = Vec::new();
    let mut out: Vec<RoutedEdge> = Vec::with_capacity(edges.len());
    for e in edges {
        let a = &nodes[e.a];
        let b = &nodes[e.b];
        let obstacles = obstacles_for(nodes, e.a, e.b);
        let routed = match best_clean_route(a, b, &obstacles, &placed) {
            Some(c) => RoutedEdge { route: c.points, sides: Some((c.side_a, c.side_b)) },
            None => RoutedEdge {
                route: forced_detour(a, b, &obstacles, &placed).unwrap_or_default(),
                sides: None,
            },
        };
        if !routed.route.is_empty() {
            placed.push(routed.route.clone());
        }
        out.push(routed);
    }
    out
}

/// Route EVERY edge: Component 1 (clean 0/1-bend) when possible, else Component 2
/// (forced monotone detour). Every route is monotone — the output has **zero
/// doglegs by construction**. Returns one polyline per edge (empty only if even a
/// monotone detour is impossible, which a layered diagram does not produce).
pub fn route_all(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    route_all_sided(nodes, edges).into_iter().map(|r| r.route).collect()
}

/// Opposite endpoint's centre coordinate along the surface tangent — the
/// co-monotone ordering key for slot assignment. Vertical surfaces (L/R) order by
/// the opposite node's y; horizontal surfaces (T/B) by its x.
fn opposite_rank(nodes: &[Rect], e: Edge, node_idx: usize, side: Side) -> f64 {
    let opp = if e.a == node_idx { e.b } else { e.a };
    let r = &nodes[opp];
    if side.is_horizontal() {
        r.y + r.height / 2.0
    } else {
        r.x + r.width / 2.0
    }
}

/// Like [`route_all`], but parallel edges sharing a surface are spread across it
/// instead of overlapping at the centre. Per surface, edges are ordered by their
/// opposite endpoint (co-monotone, to minimise intra-bundle crossings) and given
/// evenly-spaced mount slots; each clean route is rebuilt from its slotted mounts
/// (falling back to the centre route if a slot breaks feasibility). Component-2
/// detours are left as-is. Still monotone everywhere — zero doglegs.
pub fn route_all_slotted(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    let routed = route_all_sided(nodes, edges);

    // Group clean edges by the surface each end mounts: (node, side.index) -> edges.
    let mut surface: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        if let Some((sa, sb)) = routed[ei].sides {
            surface.entry((e.a, sa.index())).or_default().push(ei);
            surface.entry((e.b, sb.index())).or_default().push(ei);
        }
    }

    let side_of = |idx: u8| match idx {
        0 => Side::Left,
        1 => Side::Right,
        2 => Side::Top,
        _ => Side::Bottom,
    };
    let mut mount_a: Vec<Option<Point>> = vec![None; edges.len()];
    let mut mount_b: Vec<Option<Point>> = vec![None; edges.len()];

    // Deterministic surface order so slot assignment is reproducible.
    let mut keys: Vec<(usize, u8)> = surface.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let mut group = surface.remove(&k).unwrap();
        let (node_idx, side_idx) = k;
        let side = side_of(side_idx);
        let rect = &nodes[node_idx];
        group.sort_by(|&i, &j| {
            opposite_rank(nodes, edges[i], node_idx, side)
                .partial_cmp(&opposite_rank(nodes, edges[j], node_idx, side))
                .unwrap_or(Ordering::Equal)
                .then(i.cmp(&j))
        });
        let count = group.len();
        for (slot, &ei) in group.iter().enumerate() {
            let frac = (slot as f64 + 1.0) / (count as f64 + 1.0);
            let pt = side.mount_at(rect, frac);
            if edges[ei].a == node_idx {
                mount_a[ei] = Some(pt);
            } else {
                mount_b[ei] = Some(pt);
            }
        }
    }

    // Rebuild clean edges from slotted mounts; keep detours as-is.
    edges
        .iter()
        .enumerate()
        .map(|(ei, e)| match routed[ei].sides {
            Some((sa, sb)) => {
                let pa = mount_a[ei].clone().unwrap_or_else(|| sa.mount_at(&nodes[e.a], 0.5));
                let pb = mount_b[ei].clone().unwrap_or_else(|| sb.mount_at(&nodes[e.b], 0.5));
                let obstacles = obstacles_for(nodes, e.a, e.b);
                build_path_01(sa, &pa, sb, &pb)
                    .filter(|pts| clears(pts, &obstacles))
                    .unwrap_or_else(|| routed[ei].route.clone())
            }
            None => routed[ei].route.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_model::{bend_score, monotone};

    fn rect(x: f64, y: f64) -> Rect {
        Rect { x, y, width: 100.0, height: 50.0 }
    }

    #[test]
    fn places_a_simple_chain_cleanly() {
        // A — B — C laid left to right, same row: both edges route straight (β=0).
        let nodes = vec![rect(0.0, 0.0), rect(300.0, 0.0), rect(600.0, 0.0)];
        let edges = vec![Edge { a: 0, b: 1 }, Edge { a: 1, b: 2 }];
        let p = place_clean(&nodes, &edges);
        assert_eq!(p.clean_count, 2);
        assert_eq!(p.fallback_count, 0);
        for r in p.routes.iter().flatten() {
            assert_eq!(bend_score(r), 0.0);
        }
    }

    #[test]
    fn offset_targets_route_as_clean_ls() {
        // A in the middle, two targets diagonally placed ⇒ both clean L (β=1).
        let nodes = vec![rect(300.0, 150.0), rect(600.0, 0.0), rect(600.0, 320.0)];
        let edges = vec![Edge { a: 0, b: 1 }, Edge { a: 0, b: 2 }];
        let p = place_clean(&nodes, &edges);
        assert_eq!(p.clean_count, 2);
        for r in p.routes.iter().flatten() {
            assert!(bend_score(r) <= 1.0);
        }
    }

    #[test]
    fn route_all_routes_everything_and_is_always_monotone() {
        // A small graph; obstacles are the other nodes. Whatever each edge needs
        // (clean L or forced detour), the result is routed and never a dogleg.
        let nodes = vec![
            rect(0.0, 0.0),
            rect(400.0, 0.0),
            rect(200.0, 300.0),
            rect(600.0, 220.0),
        ];
        let edges = vec![
            Edge { a: 0, b: 1 },
            Edge { a: 0, b: 2 },
            Edge { a: 2, b: 3 },
            Edge { a: 1, b: 3 },
        ];
        let routes = route_all(&nodes, &edges);
        assert_eq!(routes.len(), 4);
        for r in &routes {
            assert!(!r.is_empty(), "every edge is routed");
            assert!(monotone(r), "every route is monotone — zero doglegs");
        }
    }

    #[test]
    fn parallel_edges_get_distinct_slots() {
        // Two parallel A→B edges must not collapse onto the same centre route —
        // slotting spreads them across the shared surfaces.
        let nodes = vec![rect(0.0, 0.0), rect(300.0, 0.0)];
        let edges = vec![Edge { a: 0, b: 1 }, Edge { a: 0, b: 1 }];
        let routes = route_all_slotted(&nodes, &edges);
        assert_eq!(routes.len(), 2);
        for r in &routes {
            assert!(!r.is_empty());
            assert!(monotone(r));
        }
        assert_ne!(routes[0], routes[1], "parallel edges land on distinct slots");
    }

    #[test]
    fn route_all_uses_component2_when_both_ls_blocked() {
        // A→B offset, two blocker nodes kill both clean Ls; route_all must still
        // route it (via Component 2) with a monotone ≥2-bend path, not a dogleg.
        let nodes = vec![
            Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 }, // A
            Rect { x: 500.0, y: 200.0, width: 100.0, height: 50.0 }, // B
            Rect { x: 200.0, y: -20.0, width: 60.0, height: 80.0 }, // blocks low L
            Rect { x: 200.0, y: 200.0, width: 60.0, height: 100.0 }, // blocks high L
        ];
        let edges = vec![Edge { a: 0, b: 1 }];
        let routes = route_all(&nodes, &edges);
        assert!(!routes[0].is_empty());
        assert!(monotone(&routes[0]));
    }

    #[test]
    fn a_node_walling_off_the_target_forces_fallback() {
        // A and B aligned, but a third node spans the entire corridor between
        // them AND extends far above/below, so no clean straight or L clears it.
        let nodes = vec![
            rect(0.0, 200.0),
            rect(600.0, 200.0),
            // tall wall covering the full vertical span between A and B's faces
            Rect { x: 250.0, y: -400.0, width: 100.0, height: 1200.0 },
        ];
        let edges = vec![Edge { a: 0, b: 1 }];
        let p = place_clean(&nodes, &edges);
        assert_eq!(p.fallback_count, 1, "walled-off edge must fall to Component 2");
        assert!(p.routes[0].is_none());
    }
}
