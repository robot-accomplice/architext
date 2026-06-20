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
use super::{bend_score, build_path_01, clears, Side, EPS, MIN_SURFACE_STEM};
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

/// The nesting surface pair for `from → to`, chosen so that a whole fan to a
/// column nests crossing-free (the coordinated fan rule). Cross-lane offset edges
/// take `from`'s horizontal face + `to`'s vertical face (a perpendicular L that
/// shares the horizontal face across the fan); same-row edges go straight; same
/// (vertical) lane uses the vertical faces. Trades length for nesting (crossings
/// ≻ length).
fn facing_pair(from: &Rect, to: &Rect) -> (Side, Side) {
    let fcx = from.x + from.width / 2.0;
    let fcy = from.y + from.height / 2.0;
    let tcx = to.x + to.width / 2.0;
    let tcy = to.y + to.height / 2.0;
    let dx = tcx - fcx;
    let dy = tcy - fcy;
    let cross_lane = dx.abs() > from.width;
    let same_row = dy.abs() <= from.height;
    let _ = same_row;
    if cross_lane {
        // Cross-lane: horizontal FACING pair. The builder makes a straight when
        // collinear, else a C (monotone jog) — keeping the whole fan on these two
        // faces so it nests, instead of relocating to a crossing-prone L.
        if dx >= 0.0 { (Side::Right, Side::Left) } else { (Side::Left, Side::Right) }
    } else {
        // Same (vertical) lane: vertical FACING pair (straight or vertical C).
        if dy >= 0.0 { (Side::Bottom, Side::Top) } else { (Side::Top, Side::Bottom) }
    }
}

/// True when two faces directly face each other (opposite outward normals).
fn is_facing(sa: Side, sb: Side) -> bool {
    let na = sa.normal();
    let nb = sb.normal();
    (na.0 + nb.0).abs() < f64::EPSILON && (na.1 + nb.1).abs() < f64::EPSILON
}

/// Clamp a jog channel so both stems (`a→channel` and `channel→b`) are at least
/// [`MIN_SURFACE_STEM`]. If the surfaces are too close to fit two stems, jog at
/// the midpoint (best effort — still monotone, never a dogleg).
fn clamp_jog(raw: f64, a: f64, b: f64) -> f64 {
    let lo = a.min(b);
    let hi = a.max(b);
    if hi - lo <= 2.0 * MIN_SURFACE_STEM {
        return (lo + hi) / 2.0;
    }
    raw.clamp(lo + MIN_SURFACE_STEM, hi - MIN_SURFACE_STEM)
}

/// Monotone C (2-bend jog) between two facing mounts, jogging at the given
/// channel (x for a horizontal facing pair, y for a vertical one). Monotone iff
/// the channel lies between the endpoints — caller ensures that — so never a
/// dogleg. `frac` staggers the jog by fan order so the Cs nest instead of sharing
/// one channel; the channel is then clamped so both stems clear
/// [`MIN_SURFACE_STEM`] (no bending right at the surface).
fn build_c(sa: Side, pa: &Point, pb: &Point, frac: f64) -> Vec<Point> {
    // Nesting: the jog channel moves toward the source as the fan order grows
    // (nearest target jogs nearest the target, farthest jogs nearest the source),
    // so the Cs nest instead of crossing — hence `1 - frac`, not `frac`.
    let t = 1.0 - frac;
    if sa.is_horizontal() {
        let jog_x = clamp_jog(pa.x + (pb.x - pa.x) * t, pa.x, pb.x);
        vec![
            pa.clone(),
            Point { x: jog_x, y: pa.y },
            Point { x: jog_x, y: pb.y },
            pb.clone(),
        ]
    } else {
        let jog_y = clamp_jog(pa.y + (pb.y - pa.y) * t, pa.y, pb.y);
        vec![
            pa.clone(),
            Point { x: pa.x, y: jog_y },
            Point { x: pb.x, y: jog_y },
            pb.clone(),
        ]
    }
}

/// Build a clean shape between two mounts on the given surfaces: straight/L via
/// [`build_path_01`], else — if the surfaces face each other — a monotone C
/// (jog) staggered by `frac`. Returns the first that clears `obstacles`.
fn build_l_or_c(
    sa: Side,
    pa: &Point,
    sb: Side,
    pb: &Point,
    frac: f64,
    obstacles: &[Rect],
) -> Option<Vec<Point>> {
    if let Some(pts) = build_path_01(sa, pa, sb, pb) {
        if clears(&pts, obstacles) {
            return Some(pts);
        }
    }
    if is_facing(sa, sb) {
        let c = build_c(sa, pa, pb, frac);
        if clears(&c, obstacles) {
            return Some(c);
        }
    }
    None
}

/// Route every edge preferring the [`facing_pair`] nesting surfaces (the
/// coordinated fan rule): straight/L when available, else a C (jog) on those same
/// facing surfaces. Falls back to [`best_clean_route`], then Component 2. Always
/// monotone — zero doglegs.
pub fn route_all_facing_sided(nodes: &[Rect], edges: &[Edge]) -> Vec<RoutedEdge> {
    let mut placed: Vec<Vec<Point>> = Vec::new();
    let mut out: Vec<RoutedEdge> = Vec::with_capacity(edges.len());
    for e in edges {
        let a = &nodes[e.a];
        let b = &nodes[e.b];
        let obstacles = obstacles_for(nodes, e.a, e.b);
        let (sa, sb) = facing_pair(a, b);
        let pa = sa.mount_at(a, 0.5);
        let pb = sb.mount_at(b, 0.5);
        let facing = build_l_or_c(sa, &pa, sb, &pb, 0.5, &obstacles);
        let routed = match facing {
            Some(pts) => RoutedEdge { route: pts, sides: Some((sa, sb)) },
            None => match best_clean_route(a, b, &obstacles, &placed) {
                Some(c) => RoutedEdge { route: c.points, sides: Some((c.side_a, c.side_b)) },
                None => RoutedEdge {
                    route: forced_detour(a, b, &obstacles, &placed).unwrap_or_default(),
                    sides: None,
                },
            },
        };
        if !routed.route.is_empty() {
            placed.push(routed.route.clone());
        }
        out.push(routed);
    }
    out
}

/// Fan router using facing surfaces with C (jog) shapes for offset-facing pairs.
///
/// MEASURED on FlowForge (2026-06-20): β 138→243, crossings 18→98 — DECISIVELY
/// WORSE. A C is 2 bends vs an L's 1, and the locked law is `bends ≻ crossings`
/// (an extra bend always loses, even to crossings). So an available 1-bend L
/// always beats a 2-bend C regardless of crossings: the L-based `route_all_slotted`
/// (138/18) is law-optimal, and the fan's remaining crossings are the mandated
/// price of bends-first — they CANNOT be removed with Cs without violating the
/// law. The C shape is therefore correct only where NO L exists (the rung between
/// L and the Component-2 staircase), never as a crossing-reducer. Kept as a
/// tested building block; not a default path.
pub fn route_all_fan(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    let routed = route_all_facing_sided(nodes, edges);
    let sides: Vec<Option<(Side, Side)>> = routed.iter().map(|r| r.sides).collect();
    let detour: Vec<Vec<Point>> = routed.iter().map(|r| r.route.clone()).collect();
    build_slotted_with_sides(nodes, edges, &sides, &detour)
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
    // Fan-order fraction of the e.a-end slot (co-monotone by target), used to
    // stagger a C's jog channel so the fan's Cs nest instead of sharing a channel.
    let mut frac_a: Vec<f64> = vec![0.5; edges.len()];
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
                frac_a[ei] = frac;
            } else {
                mount_b[ei] = Some(pt);
            }
        }
    }

    // §0 rule 6 — STRAIGHTENING PASS. Slot distribution can push a *facing* pair
    // (opposite normals) to different tangent offsets, turning a route that could
    // be a clean straight into an avoidable jog. Pull both mounts to a common
    // coordinate inside the surfaces' overlap when the straight clears O — even
    // though that breaks even spacing — guarded so the moved mount never coincides
    // with another mount on the same surface (which would overlap a reciprocal).
    let coincides = |mounts_a: &[Option<Point>], mounts_b: &[Option<Point>], ei: usize, node: usize, p: &Point| {
        edges.iter().enumerate().any(|(ej, ej_e)| {
            if ej == ei {
                return false;
            }
            let other = if ej_e.a == node {
                mounts_a[ej].as_ref()
            } else if ej_e.b == node {
                mounts_b[ej].as_ref()
            } else {
                None
            };
            other.is_some_and(|q| (q.x - p.x).abs() < EPS && (q.y - p.y).abs() < EPS)
        })
    };
    for ei in 0..edges.len() {
        let (sa, sb) = match sides[ei] {
            Some(s) => s,
            None => continue,
        };
        let (na, nb) = (sa.normal(), sb.normal());
        let facing = (na.0 + nb.0).abs() < EPS && (na.1 + nb.1).abs() < EPS;
        if !facing {
            continue;
        }
        let (pa, pb) = match (mount_a[ei].clone(), mount_b[ei].clone()) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let e = edges[ei];
        let (ra, rb) = (&nodes[e.a], &nodes[e.b]);
        let obstacles = obstacles_for(nodes, e.a, e.b);
        let (qa, qb) = if sa.is_horizontal() {
            if (pa.y - pb.y).abs() < EPS {
                continue; // already straight
            }
            let lo = ra.y.max(rb.y) + MIN_SURFACE_STEM;
            let hi = (ra.y + ra.height).min(rb.y + rb.height) - MIN_SURFACE_STEM;
            if lo > hi {
                continue; // surfaces don't overlap enough to straighten into
            }
            let c = pa.y.clamp(lo, hi);
            (Point { x: pa.x, y: c }, Point { x: pb.x, y: c })
        } else {
            if (pa.x - pb.x).abs() < EPS {
                continue;
            }
            let lo = ra.x.max(rb.x) + MIN_SURFACE_STEM;
            let hi = (ra.x + ra.width).min(rb.x + rb.width) - MIN_SURFACE_STEM;
            if lo > hi {
                continue;
            }
            let c = pa.x.clamp(lo, hi);
            (Point { x: c, y: pa.y }, Point { x: c, y: pb.y })
        };
        if !clears(&[qa.clone(), qb.clone()], &obstacles) {
            continue;
        }
        if coincides(&mount_a, &mount_b, ei, e.a, &qa) || coincides(&mount_a, &mount_b, ei, e.b, &qb) {
            continue;
        }
        mount_a[ei] = Some(qa);
        mount_b[ei] = Some(qb);
    }

    edges
        .iter()
        .enumerate()
        .map(|(ei, e)| match sides[ei] {
            Some((sa, sb)) => {
                let pa = mount_a[ei].clone().unwrap_or_else(|| sa.mount_at(&nodes[e.a], 0.5));
                let pb = mount_b[ei].clone().unwrap_or_else(|| sb.mount_at(&nodes[e.b], 0.5));
                let obstacles = obstacles_for(nodes, e.a, e.b);
                build_l_or_c(sa, &pa, sb, &pb, frac_a[ei], &obstacles)
                    .unwrap_or_else(|| detour[ei].clone())
            }
            None => detour[ei].clone(),
        })
        .collect()
}

/// Clean-shape cost weights (LAW REVISION 2026-06-20: crossings can outweigh a
/// bend). `cost = W_BEND·bends + W_CROSS·crossings`. With these, a C (2 bends)
/// beats an L (1 bend) when the L would cross ≥2 placed routes. A dogleg still
/// loses outright (`bend_score` returns `REVERSAL_BEND_PENALTY`). Tunable.
const W_BEND: f64 = 1.0;
const W_CROSS: f64 = 3.0;

/// Weighted cost of a route against the already-placed routes.
fn weighted_cost(route: &[Point], placed: &[Vec<Point>]) -> f64 {
    let b = bend_score(route);
    let x: usize = placed.iter().map(|p| polyline_crossings(route, p)).sum();
    W_BEND * b + W_CROSS * (x as f64)
}

/// Route every edge choosing, per edge, between the crossing/length-optimal L
/// ([`best_clean_route`]) and the facing-surface C, by WEIGHTED cost vs the
/// already-placed routes (the revised law — crossings can outweigh a bend). C is
/// taken only where it is weighted-cheaper, so lone/reciprocal edges keep their
/// L. Falls back to Component 2. Always monotone — zero doglegs.
pub fn route_all_weighted_sided(nodes: &[Rect], edges: &[Edge]) -> Vec<RoutedEdge> {
    let mut placed: Vec<Vec<Point>> = Vec::new();
    let mut out: Vec<RoutedEdge> = Vec::with_capacity(edges.len());
    for e in edges {
        let a = &nodes[e.a];
        let b = &nodes[e.b];
        let obstacles = obstacles_for(nodes, e.a, e.b);
        let l = best_clean_route(a, b, &obstacles, &placed);
        let (csa, csb) = facing_pair(a, b);
        let c = build_l_or_c(csa, &csa.mount_at(a, 0.5), csb, &csb.mount_at(b, 0.5), 0.5, &obstacles);
        let routed = match (l, c) {
            (Some(lc), Some(cpts)) => {
                if weighted_cost(&cpts, &placed) < weighted_cost(&lc.points, &placed) {
                    RoutedEdge { route: cpts, sides: Some((csa, csb)) }
                } else {
                    RoutedEdge { route: lc.points, sides: Some((lc.side_a, lc.side_b)) }
                }
            }
            (Some(lc), None) => RoutedEdge { route: lc.points, sides: Some((lc.side_a, lc.side_b)) },
            (None, Some(cpts)) => RoutedEdge { route: cpts, sides: Some((csa, csb)) },
            (None, None) => RoutedEdge {
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

/// Weighted-law router: per-edge L-or-C by weighted cost, then slotting. Monotone.
///
/// MEASURED on FlowForge (corrected jog): β 138→219, crossings 18→59 — still
/// worse than the L-router. The C-nesting itself works (forced-C nests clean
/// column fans to 0 crossings), but the **per-edge greedy** L/C choice is myopic:
/// it commits a C from crossings-vs-placed-so-far, while the final crossings
/// depend on slotting + the *other* C's, so it over-picks C's that don't all
/// nest. The realized win needs a **coordinated per-fan** decision (detect the
/// fan, build all-L vs all-C, compare actual nested crossings, pick per fan) —
/// the next step. Kept as a tested building block; default stays route_all_slotted.
pub fn route_all_weighted(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    let routed = route_all_weighted_sided(nodes, edges);
    let sides: Vec<Option<(Side, Side)>> = routed.iter().map(|r| r.sides).collect();
    let detour: Vec<Vec<Point>> = routed.iter().map(|r| r.route.clone()).collect();
    build_slotted_with_sides(nodes, edges, &sides, &detour)
}

/// Weighted total cost of a routing: `W_BEND·Σ shape-cost + W_CROSS·crossings`.
fn weighted_total(routes: &[Vec<Point>]) -> f64 {
    let shape: f64 = routes.iter().map(|r| bend_score(r)).sum();
    W_BEND * shape + W_CROSS * (total_crossings(routes) as f64)
}

/// Coordinated per-fan router (the non-myopic L/C choice). Start from the all-L
/// routing; group each node's edges into fans by the facing source side; then for
/// each fan toggle ALL its edges to the facing-C shape as a unit, rebuild the
/// whole diagram, and keep the toggle only if it lowers the weighted total. A
/// fan's C's therefore nest together (decided as one), avoiding the per-edge
/// greedy's myopia. Deterministic and bounded; always monotone — zero doglegs.
pub fn route_all_coordinated(nodes: &[Rect], edges: &[Edge]) -> Vec<Vec<Point>> {
    let l_routed = route_all_sided(nodes, edges);
    let c_routed = route_all_facing_sided(nodes, edges);
    let l_sides: Vec<Option<(Side, Side)>> = l_routed.iter().map(|r| r.sides).collect();
    let c_sides: Vec<Option<(Side, Side)>> = c_routed.iter().map(|r| r.sides).collect();
    let detour: Vec<Vec<Point>> = l_routed.iter().map(|r| r.route.clone()).collect();

    // Fan groups: a node's edges sharing the same facing source side.
    let mut fans: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        if l_sides[ei].is_some() {
            let fs = facing_pair(&nodes[e.a], &nodes[e.b]).0.index();
            fans.entry((e.a, fs)).or_default().push(ei);
        }
    }
    let mut fan_keys: Vec<(usize, u8)> = fans.keys().copied().collect();
    fan_keys.sort_unstable();

    let mut sides = l_sides.clone();
    let mut routes = build_slotted_with_sides(nodes, edges, &sides, &detour);
    let mut best = weighted_total(&routes);

    const MAX_ROUNDS: usize = 3;
    for _ in 0..MAX_ROUNDS {
        let mut improved = false;
        for fk in &fan_keys {
            let members = &fans[fk];
            if members.len() < 2 {
                continue; // a lone edge keeps its L (a fan needs ≥2)
            }
            // toggle the whole fan to C
            for &ei in members {
                sides[ei] = c_sides[ei];
            }
            let trial = build_slotted_with_sides(nodes, edges, &sides, &detour);
            let cost = weighted_total(&trial);
            if cost < best {
                best = cost;
                routes = trial;
                improved = true;
            } else {
                for &ei in members {
                    sides[ei] = l_sides[ei]; // revert
                }
            }
        }
        if !improved {
            break;
        }
    }

    // Phase 2: reciprocal-pair symmetry. A pair a→b / b→a routed with MISMATCHED
    // shapes (e.g. one toggled to C, its return left an L) crosses. Try forcing
    // every edge between the same node-pair onto the facing surfaces (so they run
    // parallel, same shape) and keep it if the weighted total drops.
    let mut by_pair: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        if sides[ei].is_some() {
            by_pair.entry((e.a.min(e.b), e.a.max(e.b))).or_default().push(ei);
        }
    }
    let mut pair_keys: Vec<(usize, usize)> = by_pair.keys().copied().collect();
    pair_keys.sort_unstable();
    for pk in pair_keys {
        let group = &by_pair[&pk];
        if group.len() < 2 {
            continue; // need ≥2 edges between the pair (a reciprocal/multi bundle)
        }
        let orig: Vec<Option<(Side, Side)>> = group.iter().map(|&ei| sides[ei]).collect();
        let mut changed = false;
        for &ei in group {
            if c_sides[ei].is_some() && c_sides[ei] != sides[ei] {
                sides[ei] = c_sides[ei];
                changed = true;
            }
        }
        if !changed {
            continue;
        }
        let trial = build_slotted_with_sides(nodes, edges, &sides, &detour);
        let cost = weighted_total(&trial);
        if cost < best {
            best = cost;
            routes = trial;
        } else {
            for (k, &ei) in group.iter().enumerate() {
                sides[ei] = orig[k];
            }
        }
    }

    routes
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
    use crate::route_model::{bend_score, bends, monotone};

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
    fn fan_router_routes_everything_monotone() {
        // The fan/C router must route every edge and stay dogleg-free (every route
        // monotone), whatever shapes it picks. (Whether C beats L is geometry-
        // dependent — a straddling fan nests better as Ls, an all-one-side fan as
        // Cs — so crossings are not asserted here; that is the coordinated choice.)
        let nodes = vec![
            rect(0.0, 200.0),
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
        for r in &route_all_fan(&nodes, &edges) {
            assert!(!r.is_empty(), "every edge routed");
            assert!(monotone(r), "fan routes stay dogleg-free");
        }
    }

    #[test]
    fn coordinated_never_worse_than_all_l_and_monotone() {
        // The coordinated router starts from all-L and only keeps fan→C toggles
        // that lower the weighted total, so it is never worse than route_all_slotted
        // and stays dogleg-free.
        let nodes = vec![
            rect(0.0, 200.0),
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
        let coord = route_all_coordinated(&nodes, &edges);
        let base = route_all_slotted(&nodes, &edges);
        assert!(weighted_total(&coord) <= weighted_total(&base), "coordinated never worse");
        for r in &coord {
            assert!(!r.is_empty());
            assert!(monotone(r), "still dogleg-free");
        }
    }

    #[test]
    fn build_c_is_monotone() {
        // A C jog between facing mounts never doubles back.
        let c = build_c(Side::Right, &Point { x: 0.0, y: 0.0 }, &Point { x: 100.0, y: 60.0 }, 0.5);
        assert!(monotone(&c));
        assert_eq!(bends(&c), 2, "a C has exactly 2 bends");
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
    fn route_all_arches_when_both_ls_blocked() {
        // A→B offset, two blocker nodes kill both clean Ls. The clean answer is now
        // a C arch over/around the blockers (§0) — a valid shape that never doubles
        // back over itself, not a Component-2 staircase.
        let nodes = vec![
            Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 }, // A
            Rect { x: 500.0, y: 200.0, width: 100.0, height: 50.0 }, // B
            Rect { x: 200.0, y: -20.0, width: 60.0, height: 80.0 }, // blocks low L
            Rect { x: 200.0, y: 200.0, width: 60.0, height: 100.0 }, // blocks high L
        ];
        let edges = vec![Edge { a: 0, b: 1 }];
        let routes = route_all(&nodes, &edges);
        assert!(!routes[0].is_empty());
        assert!(!crate::route_model::doubles_back(&routes[0]));
    }

    #[test]
    fn straightening_pass_aligns_a_distributed_facing_pair() {
        // A.Right carries two edges (to B and to C below it), so A→B is slotted
        // OFF-centre while B.Left (its only edge) sits centred — a facing JOG. The
        // §0 rule-6 straightening pass pulls both mounts to a common y, recovering a
        // clean straight even though that breaks even slot spacing. Without the pass
        // this edge is a 2-bend Z (offset facing surfaces).
        let nodes = vec![
            Rect { x: 0.0, y: 0.0, width: 100.0, height: 200.0 },     // A
            Rect { x: 400.0, y: 0.0, width: 100.0, height: 200.0 },   // B (same height, faces A)
            Rect { x: 400.0, y: 300.0, width: 100.0, height: 100.0 }, // C — pulls A.Right slotting
        ];
        let edges = vec![Edge { a: 0, b: 1 }, Edge { a: 0, b: 2 }];
        let sides = vec![
            Some((Side::Right, Side::Left)),
            Some((Side::Right, Side::Top)),
        ];
        let detour = vec![Vec::new(), Vec::new()];
        let routes = build_slotted_with_sides(&nodes, &edges, &sides, &detour);
        assert_eq!(
            crate::route_model::bends(&routes[0]),
            0,
            "facing pair must straighten to 0 bends, got {:?}",
            routes[0]
        );
    }

    #[test]
    fn a_wall_on_the_direct_path_is_cleared_by_an_arch() {
        // A and B aligned, but a third node spans the entire corridor between them
        // AND extends far above/below. No straight or single L clears it — but a C
        // arch goes around (over the top or under), so place_clean finds a CLEAN
        // route (§0), not a Component-2 fallback.
        let nodes = vec![
            rect(0.0, 200.0),
            rect(600.0, 200.0),
            // tall wall covering the full vertical span between A and B's faces
            Rect { x: 250.0, y: -400.0, width: 100.0, height: 1200.0 },
        ];
        let edges = vec![Edge { a: 0, b: 1 }];
        let p = place_clean(&nodes, &edges);
        assert_eq!(p.fallback_count, 0, "an arch clears the wall — no fallback needed");
        let route = p.routes[0].as_ref().expect("clean arch route");
        assert!(!crate::route_model::doubles_back(route));
    }
}
