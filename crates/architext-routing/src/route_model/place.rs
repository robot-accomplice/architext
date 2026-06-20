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
use super::select::{
    best_clean_route, clean_candidates, polyline_crossings, side_center_mounts, Candidate,
};
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

const SIDE_OF: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

/// Build slotted routes for an explicit per-edge surface assignment. Clean edges
/// (`sides[ei] = Some`) are spread across each surface (co-monotone order, even
/// slots) and rebuilt from their slotted mounts; edges with `sides[ei] = None`
/// use `detour[ei]` (their Component-2 route). Still monotone everywhere.
fn build_slotted_with_sides(
    nodes: &[Rect],
    edges: &[Edge],
    sides: &[Option<(Side, Side)>],
    detour: &[Vec<Point>],
) -> Vec<Vec<Point>> {
    let mut surface: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        if let Some((sa, sb)) = sides[ei] {
            surface.entry((e.a, sa.index())).or_default().push(ei);
            surface.entry((e.b, sb.index())).or_default().push(ei);
        }
    }
    let mut mount_a: Vec<Option<Point>> = vec![None; edges.len()];
    let mut mount_b: Vec<Option<Point>> = vec![None; edges.len()];
    let mut keys: Vec<(usize, u8)> = surface.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let mut group = surface.remove(&k).unwrap();
        let (node_idx, side_idx) = k;
        let side = SIDE_OF[side_idx as usize];
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
    edges
        .iter()
        .enumerate()
        .map(|(ei, e)| match sides[ei] {
            Some((sa, sb)) => {
                let pa = mount_a[ei].clone().unwrap_or_else(|| sa.mount_at(&nodes[e.a], 0.5));
                let pb = mount_b[ei].clone().unwrap_or_else(|| sb.mount_at(&nodes[e.b], 0.5));
                let obstacles = obstacles_for(nodes, e.a, e.b);
                build_path_01(sa, &pa, sb, &pb)
                    .filter(|pts| clears(pts, &obstacles))
                    .unwrap_or_else(|| detour[ei].clone())
            }
            None => detour[ei].clone(),
        })
        .collect()
}

/// Total strictly-interior crossings across all route pairs.
fn total_crossings(routes: &[Vec<Point>]) -> usize {
    let mut n = 0usize;
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            n += polyline_crossings(&routes[i], &routes[j]);
        }
    }
    n
}

/// Like [`route_all`], but parallel edges sharing a surface are spread across it
/// instead of overlapping (co-monotone slots). Component-2 detours unchanged.
/// Still monotone everywhere — zero doglegs.
pub fn route_all_slotted(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    let routed = route_all_sided(nodes, edges);
    let sides: Vec<Option<(Side, Side)>> = routed.iter().map(|r| r.sides).collect();
    let detour: Vec<Vec<Point>> = routed.iter().map(|r| r.route.clone()).collect();
    build_slotted_with_sides(nodes, edges, &sides, &detour)
}

/// [`route_all_slotted`] plus a crossing-aware **surface re-selection** repair:
/// for each clean edge, try every feasible clean surface pair and keep the choice
/// that lowers TOTAL slotted crossings, iterating to a fixpoint. Deterministic
/// (edge order, candidate order, first strict improvement), bounded, monotone
/// everywhere (zero doglegs), and **never increases crossings** vs plain slotting.
///
/// LIMITATION (measured on FlowForge, 2026-06-20): this single-edge greedy does
/// NOT reduce the fan-out crossing pattern (many edges from one node fanning to a
/// column). Migrating one fan edge to the facing surface just trades crossings
/// with the edges still on the original surface — a local minimum. The fan must
/// move as a COORDINATED group; that is the deferred global/eviction optimization.
/// So this is kept as a correct, never-worse refinement, not the default path.
pub fn route_all_slotted_min_crossings(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    let routed = route_all_sided(nodes, edges);
    let mut sides: Vec<Option<(Side, Side)>> = routed.iter().map(|r| r.sides).collect();
    let detour: Vec<Vec<Point>> = routed.iter().map(|r| r.route.clone()).collect();

    // Feasible clean surface pairs per clean edge (the re-selection candidates).
    let cand_sides: Vec<Vec<(Side, Side)>> = edges
        .iter()
        .enumerate()
        .map(|(ei, e)| {
            if sides[ei].is_none() {
                return Vec::new();
            }
            let obstacles = obstacles_for(nodes, e.a, e.b);
            clean_candidates(&nodes[e.a], &nodes[e.b], &obstacles)
                .into_iter()
                .map(|c| (c.side_a, c.side_b))
                .collect()
        })
        .collect();

    let mut routes = build_slotted_with_sides(nodes, edges, &sides, &detour);
    let mut best = total_crossings(&routes);
    const MAX_ROUNDS: usize = 4;
    for _ in 0..MAX_ROUNDS {
        if best == 0 {
            break;
        }
        let mut improved = false;
        for ei in 0..edges.len() {
            let orig = match sides[ei] {
                Some(s) => s,
                None => continue,
            };
            for &cand in &cand_sides[ei] {
                if cand == orig {
                    continue;
                }
                sides[ei] = Some(cand);
                let trial = build_slotted_with_sides(nodes, edges, &sides, &detour);
                let x = total_crossings(&trial);
                if x < best {
                    best = x;
                    routes = trial;
                    improved = true;
                    break;
                }
                sides[ei] = Some(orig);
            }
        }
        if !improved {
            break;
        }
    }
    routes
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
    fn min_crossings_never_worse_and_stays_monotone() {
        // A fan: one source to several offset targets. The crossing-aware repair
        // must not increase crossings vs plain slotting, and stays dogleg-free.
        let nodes = vec![
            rect(0.0, 200.0),   // source
            rect(400.0, 0.0),
            rect(400.0, 150.0),
            rect(400.0, 300.0),
            rect(400.0, 450.0),
        ];
        let edges = vec![
            Edge { a: 0, b: 1 },
            Edge { a: 0, b: 2 },
            Edge { a: 0, b: 3 },
            Edge { a: 0, b: 4 },
        ];
        let plain = total_crossings(&route_all_slotted(&nodes, &edges));
        let repaired_routes = route_all_slotted_min_crossings(&nodes, &edges);
        let repaired = total_crossings(&repaired_routes);
        assert!(repaired <= plain, "repair never increases crossings");
        for r in &repaired_routes {
            assert!(monotone(r), "still zero doglegs after repair");
        }
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
