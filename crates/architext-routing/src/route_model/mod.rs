//! Deterministic orthogonal routing model — pure geometry core (§2 of
//! `docs/architecture/ROUTING_DETERMINISTIC_MODEL.md`).
//!
//! This is the verifiable foundation of Component 1: monotonicity (the dogleg
//! test), bend counting, the β bend-score, the free-space feasibility predicate,
//! the concrete 0/1-bend path builder, and obstacle clearance. Everything here is
//! a pure function of its inputs and fully unit-tested. No eviction, no
//! Component 2 yet — those build on these primitives.

use crate::model::{Point, Rect};
use crate::route_constants::REVERSAL_BEND_PENALTY;
use crate::route_geometry::segment_intersects_rect;

/// Geometry tolerance. Coordinates are pixel-scale, so `f64::EPSILON` is far too
/// tight after arithmetic; `1e-6` separates "equal" from "different" cleanly.
pub(crate) const EPS: f64 = 1e-6;

pub mod component2;
pub mod place;
pub mod select;

/// A node surface (face), carrying an outward unit normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// Outward unit normal (screen coords, `y` increasing downward):
    /// `L=(-1,0)`, `R=(+1,0)`, `T=(0,-1)`, `B=(0,+1)`.
    pub fn normal(self) -> (f64, f64) {
        match self {
            Side::Left => (-1.0, 0.0),
            Side::Right => (1.0, 0.0),
            Side::Top => (0.0, -1.0),
            Side::Bottom => (0.0, 1.0),
        }
    }

    /// True for the horizontal-normal faces (Left/Right).
    pub fn is_horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// JS-style sign: `0` maps to `0` (not `+0`).
fn sgn(v: f64) -> i32 {
    if v > EPS {
        1
    } else if v < -EPS {
        -1
    } else {
        0
    }
}

/// §2.1 — a polyline is **monotone** (dogleg-free) iff each axis carries at most
/// one nonzero sign across its segments. A reversal (both `+` and `-` on one
/// axis) is a dogleg. A monotone polyline with ≥2 bends is a staircase.
pub fn monotone(points: &[Point]) -> bool {
    let mut sx = 0i32;
    let mut sy = 0i32;
    for w in points.windows(2) {
        let dx = sgn(w[1].x - w[0].x);
        let dy = sgn(w[1].y - w[0].y);
        if dx != 0 {
            if sx != 0 && dx != sx {
                return false;
            }
            sx = dx;
        }
        if dy != 0 {
            if sy != 0 && dy != sy {
                return false;
            }
            sy = dy;
        }
    }
    true
}

/// Number of bends (direction changes) in a polyline. Degenerate zero-length
/// segments are ignored. straight → 0, L → 1, staircase/Z (2 turns) → 2.
pub fn bends(points: &[Point]) -> usize {
    let mut count = 0usize;
    let mut prev: Option<(i32, i32)> = None;
    for w in points.windows(2) {
        let dir = (sgn(w[1].x - w[0].x), sgn(w[1].y - w[0].y));
        if dir == (0, 0) {
            continue;
        }
        if let Some(p) = prev {
            if dir != p {
                count += 1;
            }
        }
        prev = Some(dir);
    }
    count
}

/// §2.5 — the bend score β: `bends` if monotone, else `REVERSAL_BEND_PENALTY`.
/// A clean C (2 monotone bends) scores 2; a reversing Z scores 99.
pub fn bend_score(points: &[Point]) -> f64 {
    if monotone(points) {
        bends(points) as f64
    } else {
        REVERSAL_BEND_PENALTY
    }
}

/// §2.3 — free-space feasibility of a 0/1-bend route between two mounts.
/// `d = p_b - p_a`, `α = d·n_A`. Returns true iff a clean straight or single-L
/// exists in free space (clearance checked separately by [`clears`]).
pub fn free_space(side_a: Side, p_a: &Point, side_b: Side, p_b: &Point) -> bool {
    let na = side_a.normal();
    let nb = side_b.normal();
    let d = (p_b.x - p_a.x, p_b.y - p_a.y);

    // Cannot leave A's face heading back toward its interior.
    let alpha = d.0 * na.0 + d.1 * na.1;
    if alpha <= EPS {
        return false;
    }

    // Facing surfaces (n_B == -n_A): legal only if collinear (straight, 0 bends).
    if (nb.0 + na.0).abs() < EPS && (nb.1 + na.1).abs() < EPS {
        let cross = d.0 * na.1 - d.1 * na.0; // d × n_A
        return cross.abs() < EPS;
    }

    // Perpendicular surfaces (n_B ⟂ n_A): clean L iff B's face is reachable.
    if (na.0 * nb.0 + na.1 * nb.1).abs() < EPS {
        let d_nb = d.0 * nb.0 + d.1 * nb.1; // d · n_B
        return d_nb < -EPS;
    }

    // Same side (n_B == n_A): no 0/1-bend route.
    false
}

/// Build the concrete 0/1-bend polyline for a candidate, or `None` if it is not
/// free-space feasible. Facing-collinear → straight `[a,b]`; perpendicular → the
/// single L through the corner where the two axes meet.
pub fn build_path_01(side_a: Side, p_a: &Point, side_b: Side, p_b: &Point) -> Option<Vec<Point>> {
    if !free_space(side_a, p_a, side_b, p_b) {
        return None;
    }
    let na = side_a.normal();
    let nb = side_b.normal();
    let facing = (nb.0 + na.0).abs() < EPS && (nb.1 + na.1).abs() < EPS;
    if facing {
        return Some(vec![p_a.clone(), p_b.clone()]); // collinear straight
    }
    // Perpendicular L. side_a horizontal ⇒ exit horizontally ⇒ corner (p_b.x, p_a.y);
    // side_a vertical ⇒ exit vertically ⇒ corner (p_a.x, p_b.y).
    let corner = if side_a.is_horizontal() {
        Point { x: p_b.x, y: p_a.y }
    } else {
        Point { x: p_a.x, y: p_b.y }
    };
    // A degenerate corner (coincident with an endpoint) collapses to a straight.
    let on_a = (corner.x - p_a.x).abs() < EPS && (corner.y - p_a.y).abs() < EPS;
    let on_b = (corner.x - p_b.x).abs() < EPS && (corner.y - p_b.y).abs() < EPS;
    if on_a || on_b {
        Some(vec![p_a.clone(), p_b.clone()])
    } else {
        Some(vec![p_a.clone(), corner, p_b.clone()])
    }
}

/// §H3 — the polyline clears the obstacle field: no segment crosses any rect in
/// `obstacles`. The caller passes obstacles **already excluding** the two
/// endpoint nodes (whose own faces the route legitimately touches).
pub fn clears(points: &[Point], obstacles: &[Rect]) -> bool {
    for w in points.windows(2) {
        for r in obstacles {
            if segment_intersects_rect(&w[0], &w[1], r, 0.0) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn monotone_straight_l_staircase_are_monotone() {
        assert!(monotone(&[p(0.0, 0.0), p(10.0, 0.0)])); // straight
        assert!(monotone(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)])); // L
        // staircase +x,+y,+x : x always +, y +
        assert!(monotone(&[p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0), p(10.0, 5.0)]));
    }

    #[test]
    fn monotone_reversal_is_not() {
        // Z / U: +x, +y, -x  → x carries both signs ⇒ dogleg
        assert!(!monotone(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(3.0, 10.0)]));
    }

    #[test]
    fn bends_counts_turns() {
        assert_eq!(bends(&[p(0.0, 0.0), p(10.0, 0.0)]), 0); // straight
        assert_eq!(bends(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]), 1); // L
        assert_eq!(
            bends(&[p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0), p(10.0, 5.0)]),
            2
        ); // staircase
    }

    #[test]
    fn bend_score_separates_c_from_z() {
        // C: monotone 2-bend → 2
        assert_eq!(
            bend_score(&[p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0), p(10.0, 5.0)]),
            2.0
        );
        // Z: reversing 2-bend → REVERSAL_BEND_PENALTY (99)
        assert_eq!(
            bend_score(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(3.0, 10.0)]),
            REVERSAL_BEND_PENALTY
        );
        assert_eq!(bend_score(&[p(0.0, 0.0), p(10.0, 0.0)]), 0.0); // straight
        assert_eq!(bend_score(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)]), 1.0); // L
    }

    #[test]
    fn free_space_facing_only_when_collinear() {
        // A right face at (0,0), B left face to the right.
        let a = p(0.0, 0.0);
        // collinear (same y) ⇒ straight feasible
        assert!(free_space(Side::Right, &a, Side::Left, &p(20.0, 0.0)));
        // offset (different y) ⇒ infeasible (would need a Z)
        assert!(!free_space(Side::Right, &a, Side::Left, &p(20.0, 8.0)));
    }

    #[test]
    fn free_space_perpendicular_reachable_face() {
        // A right face at (0,0); B top face below-and-right ⇒ clean L.
        assert!(free_space(Side::Right, &p(0.0, 0.0), Side::Top, &p(20.0, 30.0)));
        // B top face but ABOVE A ⇒ d·n_B ≥ 0 ⇒ infeasible for a single L.
        assert!(!free_space(Side::Right, &p(0.0, 0.0), Side::Top, &p(20.0, -30.0)));
    }

    #[test]
    fn free_space_rejects_heading_away() {
        // A right face but target is to the LEFT ⇒ α ≤ 0.
        assert!(!free_space(Side::Right, &p(0.0, 0.0), Side::Top, &p(-20.0, 30.0)));
    }

    #[test]
    fn build_path_straight_and_l() {
        // facing collinear ⇒ straight
        let s = build_path_01(Side::Right, &p(0.0, 0.0), Side::Left, &p(20.0, 0.0)).unwrap();
        assert_eq!(s, vec![p(0.0, 0.0), p(20.0, 0.0)]);
        // perpendicular ⇒ L through corner (p_b.x, p_a.y)
        let l = build_path_01(Side::Right, &p(0.0, 0.0), Side::Top, &p(20.0, 30.0)).unwrap();
        assert_eq!(l, vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 30.0)]);
        assert_eq!(bends(&l), 1);
        assert!(monotone(&l));
        // infeasible ⇒ None
        assert!(build_path_01(Side::Right, &p(0.0, 0.0), Side::Left, &p(20.0, 8.0)).is_none());
    }

    #[test]
    fn clears_detects_obstacle_crossing() {
        let obstacle = Rect { x: 5.0, y: -5.0, width: 4.0, height: 20.0 };
        // straight horizontal through the obstacle ⇒ blocked
        assert!(!clears(&[p(0.0, 5.0), p(20.0, 5.0)], &[obstacle.clone()]));
        // a path that goes around (above) ⇒ clear
        assert!(clears(&[p(0.0, -10.0), p(20.0, -10.0)], &[obstacle]));
    }
}
