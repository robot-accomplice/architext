//! Component 1 — candidate enumeration and deterministic selection.
//!
//! For a node pair, enumerate every feasible clean (0/1-bend) candidate over the
//! 4×4 surfaces (mount = side centre for now; multi-edge mount slots come with
//! the placement loop), then pick `argmin_lex(β, crossings, length)` with a fixed
//! tie-break. Returns `None` when no clean candidate clears the obstacle field —
//! the signal that the connection must fall to Component 2.

use std::cmp::Ordering;

use super::{bend_score, build_path_01, clears, Side, EPS};
use crate::model::{Point, Rect};
use crate::route_geometry::route_length;

/// Deterministic side order for tie-breaking: L < R < T < B.
fn side_rank(s: Side) -> u8 {
    match s {
        Side::Left => 0,
        Side::Right => 1,
        Side::Top => 2,
        Side::Bottom => 3,
    }
}

/// The four side-centre mount candidates for a rect, in `side_rank` order.
pub fn side_center_mounts(rect: &Rect) -> [(Side, Point); 4] {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    [
        (Side::Left, Point { x: rect.x, y: cy }),
        (Side::Right, Point { x: rect.x + rect.width, y: cy }),
        (Side::Top, Point { x: cx, y: rect.y }),
        (Side::Bottom, Point { x: cx, y: rect.y + rect.height }),
    ]
}

/// A feasible clean (0/1-bend) candidate route between two nodes.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub points: Vec<Point>,
    pub side_a: Side,
    pub side_b: Side,
}

/// Enumerate every feasible clean candidate between A and B that clears the
/// obstacle field. `obstacles` must already EXCLUDE A and B (whose own faces the
/// route legitimately touches). Deterministic order: A-side then B-side.
pub fn clean_candidates(a: &Rect, b: &Rect, obstacles: &[Rect]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (sa, pa) in side_center_mounts(a) {
        for (sb, pb) in side_center_mounts(b) {
            if let Some(points) = build_path_01(sa, &pa, sb, &pb) {
                if clears(&points, obstacles) {
                    out.push(Candidate { points, side_a: sa, side_b: sb });
                }
            }
        }
    }
    out
}

/// Whether two orthogonal segments cross at a strictly-interior point (a true X,
/// not a shared endpoint or a T-touch).
fn ortho_segments_cross(a1: &Point, a2: &Point, b1: &Point, b2: &Point) -> bool {
    let a_horizontal = (a1.y - a2.y).abs() < EPS;
    let b_horizontal = (b1.y - b2.y).abs() < EPS;
    if a_horizontal == b_horizontal {
        return false; // parallel pair — no single crossing point
    }
    let (h1, h2, v1, v2) = if a_horizontal {
        (a1, a2, b1, b2)
    } else {
        (b1, b2, a1, a2)
    };
    let hy = h1.y;
    let vx = v1.x;
    let hx_lo = h1.x.min(h2.x);
    let hx_hi = h1.x.max(h2.x);
    let vy_lo = v1.y.min(v2.y);
    let vy_hi = v1.y.max(v2.y);
    vx > hx_lo + EPS && vx < hx_hi - EPS && hy > vy_lo + EPS && hy < vy_hi - EPS
}

/// Count strictly-interior crossings between two orthogonal polylines.
pub fn polyline_crossings(p: &[Point], q: &[Point]) -> usize {
    let mut n = 0usize;
    for pw in p.windows(2) {
        for qw in q.windows(2) {
            if ortho_segments_cross(&pw[0], &pw[1], &qw[0], &qw[1]) {
                n += 1;
            }
        }
    }
    n
}

/// Lexicographic cost `(β, crossings, length)` of a candidate against the
/// already-placed routes.
pub fn candidate_cost(c: &Candidate, placed: &[Vec<Point>]) -> (f64, usize, f64) {
    let beta = bend_score(&c.points);
    let crossings = placed.iter().map(|pl| polyline_crossings(&c.points, pl)).sum();
    let length = route_length(&c.points);
    (beta, crossings, length)
}

/// Pick the best clean route between A and B by `argmin_lex(β, crossings,
/// length)`, tie-broken deterministically by `(side_a, side_b)` order. Returns
/// `None` if no clean candidate clears the obstacles (caller → Component 2).
pub fn best_clean_route(
    a: &Rect,
    b: &Rect,
    obstacles: &[Rect],
    placed: &[Vec<Point>],
) -> Option<Candidate> {
    clean_candidates(a, b, obstacles).into_iter().min_by(|x, y| {
        let cx = candidate_cost(x, placed);
        let cy = candidate_cost(y, placed);
        cx.0
            .partial_cmp(&cy.0)
            .unwrap_or(Ordering::Equal)
            .then(cx.1.cmp(&cy.1))
            .then(cx.2.partial_cmp(&cy.2).unwrap_or(Ordering::Equal))
            .then(side_rank(x.side_a).cmp(&side_rank(y.side_a)))
            .then(side_rank(x.side_b).cmp(&side_rank(y.side_b)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_model::{bends, monotone};

    fn rect(x: f64, y: f64) -> Rect {
        Rect { x, y, width: 100.0, height: 50.0 }
    }

    #[test]
    fn picks_straight_for_side_by_side_same_row() {
        // B directly to the right of A, same row ⇒ a straight R→L (β=0) exists.
        let a = rect(0.0, 0.0);
        let b = rect(300.0, 0.0);
        let r = best_clean_route(&a, &b, &[], &[]).unwrap();
        assert_eq!(bend_score(&r.points), 0.0, "straight wins");
        assert_eq!(r.side_a, Side::Right);
        assert_eq!(r.side_b, Side::Left);
    }

    #[test]
    fn picks_single_l_when_offset() {
        // B down-and-right, rows offset ⇒ no straight; best is a clean L (β=1).
        let a = rect(0.0, 0.0);
        let b = rect(300.0, 200.0);
        let r = best_clean_route(&a, &b, &[], &[]).unwrap();
        assert_eq!(bend_score(&r.points), 1.0, "single L, no dogleg");
        assert!(monotone(&r.points));
        assert_eq!(bends(&r.points), 1);
    }

    #[test]
    fn obstacle_blocks_straight_forces_alternative_or_none() {
        // An obstacle squarely between A and B on the straight line. The straight
        // R→L is blocked; selection must avoid it (pick a clearing L, or None).
        let a = rect(0.0, 0.0);
        let b = rect(300.0, 0.0);
        let blocker = Rect { x: 180.0, y: -40.0, width: 40.0, height: 130.0 };
        let r = best_clean_route(&a, &b, &[blocker.clone()], &[]);
        if let Some(c) = &r {
            // whatever it picked, it must clear the blocker and never be a dogleg
            assert!(clears(&c.points, &[blocker.clone()]));
            assert!(monotone(&c.points));
            // the straight (β=0) is blocked, so any clean winner here is an L (β≥1)
            assert!(bend_score(&c.points) >= 1.0);
        }
    }

    #[test]
    fn crossing_a_placed_route_costs_more() {
        // A straight candidate; the one crossing a placed route costs +1 crossing.
        // a placed vertical route crossing the straight A→B corridor at x=150
        let placed = vec![vec![Point { x: 150.0, y: -100.0 }, Point { x: 150.0, y: 100.0 }]];
        let straight = Candidate {
            points: vec![Point { x: 100.0, y: 25.0 }, Point { x: 300.0, y: 25.0 }],
            side_a: Side::Right,
            side_b: Side::Left,
        };
        let (_b0, x0, _l0) = candidate_cost(&straight, &[]);
        let (_b1, x1, _l1) = candidate_cost(&straight, &placed);
        assert_eq!(x0, 0);
        assert_eq!(x1, 1, "straight crosses the placed vertical once");
    }

    #[test]
    fn deterministic_same_input_same_route() {
        let a = rect(10.0, 20.0);
        let b = rect(330.0, 240.0);
        let r1 = best_clean_route(&a, &b, &[], &[]).unwrap();
        let r2 = best_clean_route(&a, &b, &[], &[]).unwrap();
        assert_eq!(r1.points, r2.points);
        assert_eq!(r1.side_a, r2.side_a);
        assert_eq!(r1.side_b, r2.side_b);
    }
}
