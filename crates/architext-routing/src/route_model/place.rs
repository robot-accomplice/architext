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
use super::{bend_score, build_arch, build_path_01, clears, Side, EPS, MIN_SURFACE_STEM};
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

/// How much each successive like-facing arch pushes its crossbar further out, so
/// a bundle of arches between the same two surfaces nests instead of coinciding.
const ARCH_STAGGER: f64 = 18.0;

/// Build a clean shape between two mounts on the given surfaces. **Like-facing**
/// (`sa == sb`) builds a C arch ([`build_arch`]) — NOT `build_path_01`, whose
/// "straight" between two same-orientation faces degenerates to a line grazing the
/// shared surface plane. Otherwise straight/L via [`build_path_01`], else — if the
/// surfaces face each other — a monotone C (jog) staggered by `frac`. Returns the
/// first that clears `obstacles`.
fn build_l_or_c(
    sa: Side,
    pa: &Point,
    sb: Side,
    pb: &Point,
    frac: f64,
    obstacles: &[Rect],
) -> Option<Vec<Point>> {
    if sa == sb {
        // Stagger the crossbar by fan order so a bundle of like-facing arches nests.
        let stem = MIN_SURFACE_STEM + frac * ARCH_STAGGER;
        let arch = build_arch(sa, pa, pb, obstacles, stem);
        return arch.filter(|a| clears(a, obstacles));
    }
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

/// Over-capacity RE-FACING (the track model's capacity policy). A face holds at most
/// `floor(len / MIN_CHANNEL_CLEARANCE) - 1` mounts at ≥-arrowhead spacing (slots sit
/// at `len/(n+1)`). When a face is over-subscribed, move the most perpendicular-
/// offset excess connections to an ADJACENT face that has room — keeping the other
/// end, so the route becomes a clean perpendicular L (never a vertical-facing Z).
/// Connections that can't be cleanly re-faced stay put (accept-with-log = last
/// resort, per the maintainer). Mutates `sides`; runs before mount-ordering.
fn relieve_over_capacity(nodes: &[Rect], edges: &[Edge], sides: &mut [Option<(Side, Side)>]) {
    use crate::route_model::audit::MIN_CHANNEL_CLEARANCE;
    let cap = |node: &Rect, side: Side| -> usize {
        let len = if side.is_horizontal() { node.height } else { node.width };
        ((len / MIN_CHANNEL_CLEARANCE) as usize).saturating_sub(1).max(1)
    };
    let centre = |r: &Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    for _ in 0..edges.len() {
        // mounts per (node, side)
        let mut face: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
        for (ei, e) in edges.iter().enumerate() {
            if let Some((sa, sb)) = sides[ei] {
                face.entry((e.a, sa.index())).or_default().push(ei);
                face.entry((e.b, sb.index())).or_default().push(ei);
            }
        }
        // the most over-subscribed face
        let mut worst: Option<(usize, u8, usize)> = None;
        for (&(node, si), v) in &face {
            let over = v.len().saturating_sub(cap(&nodes[node], SIDE_OF[si as usize]));
            if over > 0 && worst.map_or(true, |(_, _, w)| over > w) {
                worst = Some((node, si, over));
            }
        }
        let (node, si, _) = match worst {
            Some(x) => x,
            None => break,
        };
        let side = SIDE_OF[si as usize];
        let nc = centre(&nodes[node]);
        // pick the excess edge with the largest perpendicular offset whose opposite
        // keeps a horizontal face (→ clean perpendicular L after the move).
        let mut best: Option<(usize, Side)> = None;
        let mut best_off = 0.0_f64;
        for &ei in &face[&(node, si)] {
            let e = edges[ei];
            let (sa, sb) = sides[ei].unwrap();
            let (opp, opp_side) = if e.a == node { (e.b, sb) } else { (e.a, sa) };
            if !opp_side.is_horizontal() {
                continue; // moving into a vertical-facing pair risks a Z
            }
            let oc = centre(&nodes[opp]);
            let (new_side, off) = if side.is_horizontal() {
                // vertical face (Left/Right) over capacity → re-face to Top/Bottom
                if oc.1 < nc.1 - EPS {
                    (Side::Top, nc.1 - oc.1)
                } else if oc.1 > nc.1 + EPS {
                    (Side::Bottom, oc.1 - nc.1)
                } else {
                    continue;
                }
            } else {
                // horizontal face (Top/Bottom) over capacity → re-face to Left/Right
                if oc.0 < nc.0 - EPS {
                    (Side::Left, nc.0 - oc.0)
                } else if oc.0 > nc.0 + EPS {
                    (Side::Right, oc.0 - nc.0)
                } else {
                    continue;
                }
            };
            if off > best_off {
                best_off = off;
                best = Some((ei, new_side));
            }
        }
        let (ei, new_side) = match best {
            Some(x) => x,
            None => break, // nothing cleanly re-faceable — accept-with-log
        };
        let e = edges[ei];
        let (sa, sb) = sides[ei].unwrap();
        sides[ei] = if e.a == node { Some((new_side, sb)) } else { Some((sa, new_side)) };
    }
}

/// Build slotted routes for an explicit per-edge surface assignment. Clean edges
/// (`sides[ei] = Some`) are spread across each surface (co-monotone order, even
/// slots) and rebuilt from their slotted mounts; edges with `sides[ei] = None`
/// use `detour[ei]` (their Component-2 route). Still monotone everywhere.
/// The default per-surface mount order: each surface's edges sorted by the
/// opposite endpoint's tangent position ([`opposite_rank`]), tie-broken by edge
/// index. This is the starting order the slot-order repair then permutes.
fn default_surface_order(
    nodes: &[Rect],
    edges: &[Edge],
    sides: &[Option<(Side, Side)>],
) -> HashMap<(usize, u8), Vec<usize>> {
    let mut surface: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
    for (ei, e) in edges.iter().enumerate() {
        if let Some((sa, sb)) = sides[ei] {
            surface.entry((e.a, sa.index())).or_default().push(ei);
            surface.entry((e.b, sb.index())).or_default().push(ei);
        }
    }
    for (k, group) in surface.iter_mut() {
        let (node_idx, side_idx) = *k;
        let side = SIDE_OF[side_idx as usize];
        group.sort_by(|&i, &j| {
            opposite_rank(nodes, edges[i], node_idx, side)
                .partial_cmp(&opposite_rank(nodes, edges[j], node_idx, side))
                .unwrap_or(Ordering::Equal)
                .then(i.cmp(&j))
        });
    }
    surface
}

fn build_slotted_with_sides(
    nodes: &[Rect],
    edges: &[Edge],
    sides: &[Option<(Side, Side)>],
    detour: &[Vec<Point>],
) -> Vec<Vec<Point>> {
    let order = default_surface_order(nodes, edges, sides);
    build_slotted_with_order(nodes, edges, sides, detour, &order)
}

/// Build slotted routes from an EXPLICIT per-surface mount order (slot = position
/// in each surface's `Vec`). [`build_slotted_with_sides`] is the default-order
/// wrapper; the slot-order repair calls this with permuted orders.
fn build_slotted_with_order(
    nodes: &[Rect],
    edges: &[Edge],
    sides: &[Option<(Side, Side)>],
    detour: &[Vec<Point>],
    order: &HashMap<(usize, u8), Vec<usize>>,
) -> Vec<Vec<Point>> {
    let mut mount_a: Vec<Option<Point>> = vec![None; edges.len()];
    let mut mount_b: Vec<Option<Point>> = vec![None; edges.len()];
    // Fan-order fraction of the e.a-end slot (co-monotone by target), used to
    // stagger a C's jog channel so the fan's Cs nest instead of sharing a channel.
    let mut frac_a: Vec<f64> = vec![0.5; edges.len()];
    let mut keys: Vec<(usize, u8)> = order.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let group = &order[&k];
        let (node_idx, side_idx) = k;
        let side = SIDE_OF[side_idx as usize];
        let rect = &nodes[node_idx];
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

    let mut routes: Vec<Vec<Point>> = edges
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
        .collect();
    routes
}

/// Hard no-overlap rule, two shape-preserving levers: nest arches whose free
/// MIDDLE channels collide, then slide a mount along its face to free any legs
/// whose PINNED channels collide. This is deterministic post-processing — it does
/// NOT change β and only converts an interlocked overlap into a permitted crossing,
/// so it runs ONCE on the final routing, never inside the mount-order optimizer
/// loop (each call scans O(E²) segment pairs; running it per trial made heavy flows
/// take minutes).
pub(crate) fn separate_all_channels(nodes: &[Rect], edges: &[Edge], routes: &mut [Vec<Point>]) {
    separate_channels(routes);
    separate_leg_channels(nodes, edges, routes);
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

/// Count of channel overlaps: pairs of segments from DIFFERENT routes that are
/// collinear and overlap (share a run, not just a crossing point). A measurement
/// helper — the maintainer's hard rule is that this must be ZERO, but it is
/// enforced by a shape-preserving channel-separation pass, NOT by penalising it in
/// the shape optimizer (that just trades an overlap for a forbidden Z).
pub(crate) fn total_overlaps(routes: &[Vec<Point>]) -> usize {
    let mut n = 0usize;
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            for wi in routes[i].windows(2) {
                for wj in routes[j].windows(2) {
                    if crate::route_model::segments_overlap(&wi[0], &wi[1], &wj[0], &wj[1]) {
                        n += 1;
                    }
                }
            }
        }
    }
    n
}

/// Minimum clear gap between two parallel channels — an arrowhead wide — so two
/// lines sharing a band never touch (the hard no-overlap rule).
const CHANNEL_GAP: f64 = 12.0;

/// The free conjoining (middle) segment of a clean 2-bend C/arch route
/// `[pa, m1, m2, pb]`: its axis (the channel the line owns), the segment's extent
/// along that axis, and the **outward** direction (the side the arch bulges, away
/// from both mounts). `None` for non-arch shapes (straight/L, Z, or any route whose
/// two end segments are NOT on the same side of the middle — only a true C/arch has
/// a freely-shiftable channel).
struct MiddleChannel {
    horiz: bool, // middle segment runs horizontally (an over/under arch)
    off: f64,    // the axis coordinate of the channel (y if horiz, else x)
    lo: f64,     // extent of the middle segment along its run
    hi: f64,
    out: f64, // +1/-1: direction the arch bulges away from the surface
}

fn middle_channel(r: &[Point]) -> Option<MiddleChannel> {
    if r.len() != 4 {
        return None; // only clean 2-bend C/arch routes have a free middle channel
    }
    let (pa, m1, m2, pb) = (&r[0], &r[1], &r[2], &r[3]);
    if (m1.y - m2.y).abs() < EPS && (m1.x - m2.x).abs() >= EPS {
        // horizontal middle at y = m1.y; both stems must hang off the SAME side
        let off = m1.y;
        let (da, db) = (pa.y - off, pb.y - off);
        if da.abs() < EPS || db.abs() < EPS || da.signum() != db.signum() {
            return None; // grazes the channel or is a Z — not a true arch
        }
        Some(MiddleChannel { horiz: true, off, lo: m1.x.min(m2.x), hi: m1.x.max(m2.x), out: -da.signum() })
    } else if (m1.x - m2.x).abs() < EPS && (m1.y - m2.y).abs() >= EPS {
        let off = m1.x;
        let (da, db) = (pa.x - off, pb.x - off);
        if da.abs() < EPS || db.abs() < EPS || da.signum() != db.signum() {
            return None;
        }
        Some(MiddleChannel { horiz: false, off, lo: m1.y.min(m2.y), hi: m1.y.max(m2.y), out: -da.signum() })
    } else {
        None
    }
}

/// Rebuild a 2-bend arch route with its channel shifted to `off` (its mounts and
/// shape class unchanged — only the conjoining segment moves, stems stretch).
fn reseat_middle(r: &mut [Point], horiz: bool, off: f64) {
    if horiz {
        r[1].y = off;
        r[2].y = off;
    } else {
        r[1].x = off;
        r[2].x = off;
    }
}

/// Shape-preserving channel separation — the seed of the maintainer's track model.
/// Two arch routes whose conjoining segments share a channel (collinear AND
/// overlapping) violate the hard no-overlap rule. Cluster them by that exact
/// overlap, then nest concentrically: the **widest-span** arch owns the OUTER
/// channel, narrower arches nest inside it, each ≥ [`CHANNEL_GAP`] apart. Only the
/// free middle offset moves (stems lengthen), so the §0 shape class is untouched —
/// no bend is added, and a properly contained bundle gains no crossing. Penalising
/// overlap in the optimizer would instead buy separation with forbidden Z's; this
/// pass keeps the shapes and separates the geometry.
fn separate_channels(routes: &mut [Vec<Point>]) {
    let mids: Vec<(usize, MiddleChannel)> = routes
        .iter()
        .enumerate()
        .filter_map(|(i, r)| middle_channel(r).map(|m| (i, m)))
        .collect();
    if mids.len() < 2 {
        return;
    }
    // Union-find clusters of arches sharing a channel (collinear + overlapping
    // middles — exactly what `segments_overlap` detects, the overlap metric itself).
    let n = mids.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while parent[r] != r {
            r = parent[r];
        }
        let mut c = x;
        while parent[c] != c {
            let next = parent[c];
            parent[c] = r;
            c = next;
        }
        r
    }
    for i in 0..n {
        for j in (i + 1)..n {
            let mid = |k: usize| {
                let (a, b) = (&routes[mids[k].0][1], &routes[mids[k].0][2]);
                (a.clone(), b.clone())
            };
            let (a0, a1) = mid(i);
            let (b0, b1) = mid(j);
            // Cluster middles that share a channel — collinear+overlapping (exact),
            // OR close-parallel: same orientation, extents overlap, and offsets within
            // a CHANNEL_GAP. The concentric nesting below then spreads the whole
            // cluster to ≥ an arrowhead apart (the channel-buffer rule for arches).
            let (ci, cj) = (&mids[i].1, &mids[j].1);
            let close = ci.horiz == cj.horiz
                && ci.lo.max(cj.lo) + EPS < ci.hi.min(cj.hi)
                && (ci.off - cj.off).abs() < CHANNEL_GAP;
            if close || crate::route_model::segments_overlap(&a0, &a1, &b0, &b1) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        clusters.entry(root).or_default().push(i);
    }
    let mut roots: Vec<usize> = clusters.keys().copied().collect();
    roots.sort_unstable();
    for root in roots {
        let members = &clusters[&root];
        if members.len() < 2 {
            continue;
        }
        // Nest narrow → wide. The narrowest keeps the innermost (current) channel;
        // each wider arch is pushed one CHANNEL_GAP further out than the previous.
        let mut order: Vec<usize> = members.clone();
        order.sort_by(|&a, &b| {
            let wa = mids[a].1.hi - mids[a].1.lo;
            let wb = mids[b].1.hi - mids[b].1.lo;
            wa.partial_cmp(&wb).unwrap_or(Ordering::Equal).then(mids[a].0.cmp(&mids[b].0))
        });
        let out = mids[order[0]].1.out;
        let mut off = mids[order[0]].1.off;
        for (k, &mi) in order.iter().enumerate() {
            let (ri, m) = (&mids[mi].0, &mids[mi].1);
            if k > 0 {
                off += out * CHANNEL_GAP;
                reseat_middle(&mut routes[*ri], m.horiz, off);
            }
        }
        // LINE ORDERING: nesting the middles isn't enough when several arches mount
        // the SAME face on one end — if that end's mount order doesn't match the
        // nesting, the outer arch's stem cuts across the inner middles (a 3-surface
        // bundle still nests with the right order). Reorder the tightly-clustered
        // (shared-face) end's mounts to MIRROR the other (fixed) end, giving nested
        // intervals and no stem crossing. Guarded: kept only if total crossings drop.
        let arches: Vec<usize> = order.iter().map(|&mi| mids[mi].0).collect();
        let vertical_mid = order.iter().all(|&mi| !mids[mi].1.horiz);
        if arches.len() >= 2 && vertical_mid && arches.iter().all(|&ri| routes[ri].len() == 4) {
            let end_y = |ri: usize, top: bool| if top { routes[ri][0].y } else { routes[ri][3].y };
            let end_x = |ri: usize, top: bool| if top { routes[ri][0].x } else { routes[ri][3].x };
            let same_x = |top: bool| arches.iter().all(|&ri| (end_x(ri, top) - end_x(arches[0], top)).abs() < EPS);
            let spread = |top: bool| {
                let ys: Vec<f64> = arches.iter().map(|&ri| end_y(ri, top)).collect();
                ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min)
            };
            // shared end = same x and the tighter tangent spread (one face, not many)
            let shared_top = if same_x(true) && (!same_x(false) || spread(true) <= spread(false)) {
                Some(true)
            } else if same_x(false) {
                Some(false)
            } else {
                None
            };
            if let Some(top) = shared_top {
                let mut slots: Vec<f64> = arches.iter().map(|&ri| end_y(ri, top)).collect();
                slots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
                let mut idx: Vec<usize> = (0..arches.len()).collect();
                idx.sort_by(|&a, &b| {
                    end_y(arches[a], !top).partial_cmp(&end_y(arches[b], !top)).unwrap_or(Ordering::Equal)
                });
                let saved: Vec<Vec<Point>> = arches.iter().map(|&ri| routes[ri].clone()).collect();
                let before = total_crossings(routes);
                for (k, &ai) in idx.iter().enumerate() {
                    let ri = arches[ai];
                    let new_y = slots[slots.len() - 1 - k]; // mirror: asc other-end → desc this-end
                    let (m, c) = if top { (0, 1) } else { (3, 2) };
                    routes[ri][m].y = new_y;
                    routes[ri][c].y = new_y;
                }
                if total_crossings(routes) >= before {
                    for (j, &ri) in arches.iter().enumerate() {
                        routes[ri] = saved[j].clone();
                    }
                }
            }
        }
    }
}

/// A leg = the segment incident to a mount. Unlike an arch's interior middle, a
/// leg's channel is PINNED to its mount slot, so it can only be freed by sliding
/// the mount ALONG its face. This records where the mount may slide.
struct Leg {
    ri: usize,       // route index
    mount_i: usize,  // polyline index of the mount end of the leg
    corner_i: usize, // polyline index of the leg's other end (the corner)
    vertical: bool,  // leg runs vertically (mount on a top/bottom face) → slide x
    lo: f64,         // face span the mount may slide within
    hi: f64,
}

fn collect_legs(nodes: &[Rect], edges: &[Edge], routes: &[Vec<Point>]) -> Vec<Leg> {
    let mut legs = Vec::new();
    let mut push = |ri: usize, mi: usize, ci: usize, node: &Rect, out: &mut Vec<Leg>| {
        let (m, c) = (&routes[ri][mi], &routes[ri][ci]);
        if (m.x - c.x).abs() < EPS && (m.y - c.y).abs() >= EPS {
            out.push(Leg { ri, mount_i: mi, corner_i: ci, vertical: true, lo: node.x, hi: node.x + node.width });
        } else if (m.y - c.y).abs() < EPS && (m.x - c.x).abs() >= EPS {
            out.push(Leg { ri, mount_i: mi, corner_i: ci, vertical: false, lo: node.y, hi: node.y + node.height });
        }
    };
    for (ri, r) in routes.iter().enumerate() {
        if r.len() < 3 {
            continue; // a 2-point straight has no corner to absorb a slide
        }
        let e = edges[ri];
        push(ri, 0, 1, &nodes[e.a], &mut legs);
        let last = r.len() - 1;
        push(ri, last, last - 1, &nodes[e.b], &mut legs);
    }
    legs
}

fn leg_channel(routes: &[Vec<Point>], leg: &Leg) -> f64 {
    let m = &routes[leg.ri][leg.mount_i];
    if leg.vertical { m.x } else { m.y }
}

/// Shape-preserving LEG channel separation — the pinned-channel companion to
/// [`separate_channels`]. Two legs sharing a channel (collinear AND overlapping)
/// can't be freed by an interior shift, so slide ONE mount along its face by a
/// [`CHANNEL_GAP`]: the leg moves to a clear parallel channel and its perpendicular
/// stem absorbs the shift, so a clean L stays an L (no new bend). GUARDED: a slide
/// is kept only if it strictly lowers `total_overlaps` without adding a crossing,
/// and only if the moved mount stays on its face — so it can never regress. This is
/// the deterministic seed of the track model's slot-side channel ownership.
fn separate_leg_channels(nodes: &[Rect], edges: &[Edge], routes: &mut [Vec<Point>]) {
    use crate::route_model::audit::{parallel_too_close, total_tight_pairs, MIN_CHANNEL_CLEARANCE};
    let mut base_ov = total_overlaps(routes);
    let mut base_tight = total_tight_pairs(routes);
    if base_ov == 0 && base_tight == 0 {
        return;
    }
    let legs = collect_legs(nodes, edges, routes);
    let seg = |routes: &[Vec<Point>], l: &Leg| (routes[l.ri][l.mount_i].clone(), routes[l.ri][l.corner_i].clone());
    // Apply a leg's slide in place, returning the saved endpoints for revert.
    let apply = |routes: &mut [Vec<Point>], l: &Leg, next: f64| -> (Point, Point) {
        let saved = (routes[l.ri][l.mount_i].clone(), routes[l.ri][l.corner_i].clone());
        if l.vertical {
            routes[l.ri][l.mount_i].x = next;
            routes[l.ri][l.corner_i].x = next;
        } else {
            routes[l.ri][l.mount_i].y = next;
            routes[l.ri][l.corner_i].y = next;
        }
        saved
    };
    for i in 0..legs.len() {
        for j in (i + 1)..legs.len() {
            if legs[i].ri == legs[j].ri {
                continue;
            }
            let (a0, a1) = seg(routes, &legs[i]);
            let (b0, b1) = seg(routes, &legs[j]);
            let overlapping = crate::route_model::segments_overlap(&a0, &a1, &b0, &b1);
            let close = parallel_too_close(&a0, &a1, &b0, &b1, MIN_CHANNEL_CLEARANCE);
            if !overlapping && !close {
                continue;
            }
            // Try sliding either leg by ±CHANNEL_GAP and KEEP the candidate that
            // minimises (overlaps, tight, crossings) lexicographically. An OVERLAP is
            // strictly worse than a TIGHT channel, which is strictly worse than a
            // CROSSING (lines may meet orthogonally but never share a channel or run
            // closer than an arrowhead). So this both separates exact overlaps and
            // opens close-parallel runs to >= an arrowhead, accepting a permitted
            // crossing only when nothing better fits. Shape is untouched (the mount
            // slides along its face), so no bend is ever added.
            let mut best: Option<(usize, usize, usize, f64)> = None; // (ov, tight, cr, next)
            let mut best_leg: Option<&Leg> = None;
            for li in [&legs[i], &legs[j]] {
                for &d in &[CHANNEL_GAP, -CHANNEL_GAP] {
                    let next = leg_channel(routes, li) + d;
                    if next < li.lo + EPS || next > li.hi - EPS {
                        continue; // would slide off the face
                    }
                    let saved = apply(routes, li, next);
                    let (ov, tt, cr) = (total_overlaps(routes), total_tight_pairs(routes), total_crossings(routes));
                    routes[li.ri][li.mount_i] = saved.0;
                    routes[li.ri][li.corner_i] = saved.1;
                    if (ov, tt) < (base_ov, base_tight)
                        && best.is_none_or(|(bo, bt, bc, _)| (ov, tt, cr) < (bo, bt, bc))
                    {
                        best = Some((ov, tt, cr, next));
                        best_leg = Some(li);
                    }
                }
            }
            if let (Some((ov, tt, _, next)), Some(li)) = (best, best_leg) {
                apply(routes, li, next);
                base_ov = ov;
                base_tight = tt;
            }
            if base_ov == 0 && base_tight == 0 {
                return;
            }
        }
    }
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

    // Over-capacity re-facing: relieve faces with more mounts than fit at
    // ≥-arrowhead spacing by moving excess connections to an adjacent face.
    relieve_over_capacity(nodes, edges, &mut sides);

    // Phase 3: MOUNT-ORDER repair (lane nesting). A bundle of edges sharing a
    // surface crosses when its slot order doesn't nest with the partner surface's
    // order — `opposite_rank` ties (same opposite node) fall back to edge index,
    // which is geometry-blind. Adjacent-swap local search over each surface's slot
    // order, keeping a swap only when the weighted total drops. Provably
    // non-increasing; shape is untouched (slots only), so it never adds a bend.
    let mut order = default_surface_order(nodes, edges, &sides);
    routes = build_slotted_with_order(nodes, edges, &sides, &detour, &order);
    best = weighted_total(&routes);
    for _ in 0..MAX_ROUNDS {
        let mut improved = false;
        let mut keys: Vec<(usize, u8)> = order.keys().copied().collect();
        keys.sort_unstable();
        for k in keys {
            let n = order[&k].len();
            if n < 2 {
                continue;
            }
            for s in 0..n - 1 {
                order.get_mut(&k).unwrap().swap(s, s + 1);
                let trial = build_slotted_with_order(nodes, edges, &sides, &detour, &order);
                let cost = weighted_total(&trial);
                if cost < best {
                    best = cost;
                    routes = trial;
                    improved = true;
                } else {
                    order.get_mut(&k).unwrap().swap(s, s + 1); // revert
                }
            }
        }
        if !improved {
            break;
        }
    }

    // Phase 3b: JOINT pair-swap (track nesting). A reciprocal / multi-edge bundle
    // between two surfaces nests only when both surfaces' slot orders move together
    // — but the right "mirror" can be the SAME order (facing surfaces) or the
    // REVERSED order (like-facing ∩/∪ arches between a left and a right node, the
    // outermost track spanning unified-leftmost ↔ sql-rightmost). So for each edge
    // pair we try swapping their order on EACH shared surface alone (reaches the
    // reversed mirror) and on ALL shared at once (the same-order mirror), keeping
    // whichever lowers the weighted total. Slots only — never adds a bend.
    let swap_in = |order: &mut HashMap<(usize, u8), Vec<usize>>, ks: &[(usize, u8)], ei: usize, ej: usize| {
        for k in ks {
            let g = order.get_mut(k).unwrap();
            let pi = g.iter().position(|&x| x == ei).unwrap();
            let pj = g.iter().position(|&x| x == ej).unwrap();
            g.swap(pi, pj);
        }
    };
    for _ in 0..MAX_ROUNDS {
        let mut improved = false;
        for ei in 0..edges.len() {
            for ej in (ei + 1)..edges.len() {
                let shared: Vec<(usize, u8)> = order
                    .iter()
                    .filter(|(_, g)| g.contains(&ei) && g.contains(&ej))
                    .map(|(k, _)| *k)
                    .collect();
                if shared.is_empty() {
                    continue;
                }
                // candidate moves: each shared surface alone, then all together.
                let mut moves: Vec<Vec<(usize, u8)>> = shared.iter().map(|k| vec![*k]).collect();
                if shared.len() > 1 {
                    moves.push(shared.clone());
                }
                for mv in moves {
                    swap_in(&mut order, &mv, ei, ej);
                    let trial = build_slotted_with_order(nodes, edges, &sides, &detour, &order);
                    let cost = weighted_total(&trial);
                    if cost < best {
                        best = cost;
                        routes = trial;
                        improved = true;
                        break;
                    }
                    swap_in(&mut order, &mv, ei, ej); // revert
                }
            }
        }
        if !improved {
            break;
        }
    }

    // Hard no-overlap rule: applied ONCE on the chosen routing (shape-preserving),
    // not inside the mount-order optimizer above.
    separate_all_channels(nodes, edges, &mut routes);
    routes
}

/// Total strictly-interior crossings across all route pairs.
pub(crate) fn total_crossings(routes: &[Vec<Point>]) -> usize {
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
    fn two_legs_in_a_shared_gutter_own_distinct_channels() {
        // Two L routes whose vertical LEGS land in the same x-channel and overlap in
        // y — the FlowForge interactive-turn/system-map residual. U→R1 drops down the
        // gutter past the midline; Lo→R2 climbs the SAME gutter past it, so the legs
        // share x=260 over y[100,250]. A leg's channel is PINNED to its mount slot, so
        // separation slides a mount ALONG its face (the perpendicular stem absorbs the
        // shift) — still a clean L, no new bend. These routes are INTERLOCKED (their
        // corners swap vertical order), so the only legal resolution is to convert the
        // forbidden overlap into a PERMITTED orthogonal crossing (lines may meet
        // orthogonally, but never share a channel).
        let nodes = vec![
            Rect { x: 200.0, y: 0.0, width: 120.0, height: 50.0 },   // U  bottom face y=50, x[200,320]
            Rect { x: 200.0, y: 300.0, width: 120.0, height: 50.0 }, // Lo top face y=300, x[200,320]
            Rect { x: 500.0, y: 225.0, width: 120.0, height: 50.0 }, // R1 left mount y=250 (below mid)
            Rect { x: 500.0, y: 75.0, width: 120.0, height: 50.0 },  // R2 left mount y=100 (above mid)
        ];
        let edges = vec![Edge { a: 0, b: 2 }, Edge { a: 1, b: 3 }];
        let sides = vec![Some((Side::Bottom, Side::Left)), Some((Side::Top, Side::Left))];
        let detour = vec![Vec::new(), Vec::new()];
        let mut routes = build_slotted_with_sides(&nodes, &edges, &sides, &detour);
        separate_all_channels(&nodes, &edges, &mut routes);
        for r in &routes {
            assert!(!r.is_empty(), "both Ls build");
            assert!(!crate::route_model::doubles_back(r), "shape preserved (no dogleg)");
            assert_eq!(crate::route_model::bends(r), 1, "still a clean L");
        }
        assert_eq!(
            total_overlaps(&routes),
            0,
            "two legs must not share a gutter channel; routes = {:?}",
            routes
        );
        assert!(
            total_crossings(&routes) <= 1,
            "an interlocked overlap resolves to at most ONE permitted orthogonal crossing; routes = {:?}",
            routes
        );
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
    fn two_like_facing_arches_on_a_plane_own_distinct_channels() {
        // Two like-facing (Top,Top) edges between coplanar nodes both arch over the
        // top. Each is the sole edge on its own surface, so both get the same fan
        // fraction → the same crossbar stem → the same channel. With overlapping
        // x-spans their crossbars coincide, violating the hard no-overlap rule. The
        // builder must separate them onto distinct, concentrically-nested channels
        // WITHOUT re-bending (still clean arches, zero doublings-back).
        // A→B spans the wider top channel; C→D nests inside it (its mounts fall
        // within A→B's crossbar x-range), the real FlowForge shape ([435,900] ⊃
        // [480,855]). Concentric nesting (wider arch outermost) separates them with
        // no new crossing.
        let nodes = vec![
            Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 },   // A  top-centre x=50
            Rect { x: 500.0, y: 0.0, width: 100.0, height: 50.0 }, // B  top-centre x=550
            Rect { x: 100.0, y: 0.0, width: 100.0, height: 50.0 }, // C  top-centre x=150
            Rect { x: 400.0, y: 0.0, width: 100.0, height: 50.0 }, // D  top-centre x=450
        ];
        let edges = vec![Edge { a: 0, b: 1 }, Edge { a: 2, b: 3 }];
        let sides = vec![Some((Side::Top, Side::Top)), Some((Side::Top, Side::Top))];
        let detour = vec![Vec::new(), Vec::new()];
        let mut routes = build_slotted_with_sides(&nodes, &edges, &sides, &detour);
        separate_all_channels(&nodes, &edges, &mut routes);
        for r in &routes {
            assert!(!r.is_empty(), "both arches build");
            assert!(!crate::route_model::doubles_back(r), "shape preserved (no dogleg)");
            assert_eq!(crate::route_model::bends(r), 2, "still a clean 2-bend arch");
        }
        assert_eq!(
            total_overlaps(&routes),
            0,
            "two like-facing arches must not share a channel; routes = {:?}",
            routes
        );
        assert_eq!(
            total_crossings(&routes),
            0,
            "concentric nesting adds no crossing; routes = {:?}",
            routes
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
