//! Deterministic orthogonal routing model — pure geometry core (§2 of
//! `docs/architecture/ROUTING_DETERMINISTIC_MODEL.md`).
//!
//! This is the verifiable foundation of Component 1: monotonicity (the dogleg
//! test), bend counting, the β bend-score, the free-space feasibility predicate,
//! the concrete 0/1-bend path builder, and obstacle clearance. Everything here is
//! a pure function of its inputs and fully unit-tested. No eviction, no
//! Component 2 yet — those build on these primitives.

use crate::model::{Point, Rect};
use crate::route_geometry::segment_intersects_rect;

/// Geometry tolerance. Coordinates are pixel-scale, so `f64::EPSILON` is far too
/// tight after arithmetic; `1e-6` separates "equal" from "different" cleanly.
pub(crate) const EPS: f64 = 1e-6;

/// The most bends a *clean* shape may have: a C (the monotone 2-bend jog). The
/// shape ladder is straight 0, L 1, C 2.
pub(crate) const MAX_CLEAN_BENDS: usize = 2;

/// A Z formation / staircase — monotone, but with more bends than a C (≥3). Never
/// acceptable, because distance is always preferred to a 3rd bend.
pub(crate) const Z_PENALTY: f64 = 99.0;

/// A dogleg — a line that doubles back over itself (reverses heading on an axis).
/// Categorically forbidden: priced so far past every other shape that no trade of
/// crossings or length can ever justify one.
pub(crate) const DOGLEG_PENALTY: f64 = 1_000_000_000.0;

/// Minimum straight run a route must travel off a surface before its first bend
/// (no bending right at the wall), and the gap an arch crossbar leaves past the
/// obstacles it clears. Maintainer rule, 2026-06-20.
pub(crate) const MIN_SURFACE_STEM: f64 = 16.0;

pub mod audit;
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

    /// Deterministic order key: L < R < T < B.
    pub fn index(self) -> u8 {
        match self {
            Side::Left => 0,
            Side::Right => 1,
            Side::Top => 2,
            Side::Bottom => 3,
        }
    }

    /// Mount point at fraction `frac ∈ [0,1]` along this surface of `rect`. The
    /// tangent axis is y for Left/Right, x for Top/Bottom; `0.5` is the centre.
    pub fn mount_at(self, rect: &Rect, frac: f64) -> Point {
        match self {
            Side::Left => Point { x: rect.x, y: rect.y + rect.height * frac },
            Side::Right => Point { x: rect.x + rect.width, y: rect.y + rect.height * frac },
            Side::Top => Point { x: rect.x + rect.width * frac, y: rect.y },
            Side::Bottom => Point { x: rect.x + rect.width * frac, y: rect.y + rect.height },
        }
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

/// The **corners** of a polyline: endpoints plus every turning point, with
/// collinear/degenerate intermediate points removed. A straight has 2 corners, an
/// L 3, a C/Z 4, a staircase ≥5. `corners.len() - 2` is the bend count.
pub fn corners(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        match out.last() {
            Some(last) if (last.x - p.x).abs() < EPS && (last.y - p.y).abs() < EPS => {}
            _ => out.push(p.clone()),
        }
    }
    // drop interior points that don't turn (collinear with both neighbours)
    let mut i = 1;
    while out.len() >= 3 && i < out.len() - 1 {
        let a = &out[i - 1];
        let b = &out[i];
        let c = &out[i + 1];
        let d1 = (sgn(b.x - a.x), sgn(b.y - a.y));
        let d2 = (sgn(c.x - b.x), sgn(c.y - b.y));
        if d1 == d2 {
            out.remove(i);
        } else {
            i += 1;
        }
    }
    out
}

/// Number of bends (direction changes) in a polyline. straight → 0, L → 1,
/// C/Z → 2, staircase → ≥3.
pub fn bends(points: &[Point]) -> usize {
    corners(points).len().saturating_sub(2)
}

/// Whether two collinear axis-aligned segments **overlap** (share more than a
/// single point on the same line) — the geometric signature of a line lying over
/// itself, OR of two different routes sharing a channel.
pub(crate) fn segments_overlap(a0: &Point, a1: &Point, b0: &Point, b1: &Point) -> bool {
    let a_horiz = (a0.y - a1.y).abs() < EPS;
    let b_horiz = (b0.y - b1.y).abs() < EPS;
    if a_horiz != b_horiz {
        return false; // perpendicular — cannot overlap collinearly
    }
    if a_horiz {
        if (a0.y - b0.y).abs() >= EPS {
            return false; // different horizontal lines
        }
        let (alo, ahi) = (a0.x.min(a1.x), a0.x.max(a1.x));
        let (blo, bhi) = (b0.x.min(b1.x), b0.x.max(b1.x));
        alo.max(blo) + EPS < ahi.min(bhi)
    } else {
        if (a0.x - b0.x).abs() >= EPS {
            return false;
        }
        let (alo, ahi) = (a0.y.min(a1.y), a0.y.max(a1.y));
        let (blo, bhi) = (b0.y.min(b1.y), b0.y.max(b1.y));
        alo.max(blo) + EPS < ahi.min(bhi)
    }
}

/// §0 — a polyline is a **dogleg** iff it doubles back **over itself**: two of its
/// segments are collinear and overlap, i.e. the line is drawn on top of a part of
/// itself. This is the literal "doubles back over itself" — NOT a coordinate-axis
/// sign reversal (a clean C reverses heading on a free direction yet never folds
/// over itself, so it is never a dogleg).
pub fn doubles_back(points: &[Point]) -> bool {
    let c = corners(points);
    if c.len() < 3 {
        return false;
    }
    for i in 0..c.len() - 1 {
        for j in (i + 1)..c.len() - 1 {
            if segments_overlap(&c[i], &c[i + 1], &c[j], &c[j + 1]) {
                return true;
            }
        }
    }
    false
}

/// §0 — is a **two-bend** route a **C** (`true`) or a **Z** (`false`)? Decided by
/// which side of the **middle (conjoining) segment** the two end segments sit:
/// **same side → C** (`[ ] ∩ ∪`, two like-facing surfaces); **opposite sides → Z**
/// (`_|ˉ`, the jog between two facing surfaces). `c` must be the 4-corner form.
fn two_bend_is_c(c: &[Point]) -> bool {
    debug_assert_eq!(c.len(), 4);
    let (a, p1, p2, b) = (&c[0], &c[1], &c[2], &c[3]);
    if (p1.y - p2.y).abs() < EPS {
        // middle segment horizontal at y = p1.y: same side ⇔ A and B both above or
        // both below it.
        sgn(a.y - p1.y) == sgn(b.y - p2.y)
    } else {
        // middle segment vertical at x = p1.x.
        sgn(a.x - p1.x) == sgn(b.x - p2.x)
    }
}

/// §2.5 — the bend score β (the §0 shape ladder). **Shape, not bend count.**
/// A **dogleg** (the line folds over itself, [`doubles_back`]) scores
/// [`DOGLEG_PENALTY`] (1e9). Otherwise by shape: straight 0, L 1; a two-bend route
/// is a **C** (2) when its end segments sit on the **same side** of the middle
/// segment, else a **Z** ([`Z_PENALTY`], 99); any route with ≥3 bends is a
/// **staircase** (also [`Z_PENALTY`], 99).
pub fn bend_score(points: &[Point]) -> f64 {
    if doubles_back(points) {
        return DOGLEG_PENALTY;
    }
    let c = corners(points);
    match c.len() {
        0..=2 => 0.0,                                      // straight
        3 => 1.0,                                              // L
        4 if two_bend_is_c(&c) => MAX_CLEAN_BENDS as f64,      // C
        _ => Z_PENALTY,                                        // Z / staircase
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

/// Build a **C arch** between two **like-facing** mounts (both on `side`). Both
/// stems leave along `side`'s outward normal to a shared crossbar placed `stem`
/// beyond every obstacle in the span, then return — `∩` (both Top), `∪` (both
/// Bottom), `[` (both Left), `]` (both Right). Four points, two bends, end
/// segments on the **same side** of the crossbar ⇒ a C by §0. Returns `None` only
/// for a degenerate (coincident) mount pair.
pub fn build_arch(
    side: Side,
    p_a: &Point,
    p_b: &Point,
    obstacles: &[Rect],
    stem: f64,
) -> Option<Vec<Point>> {
    if (p_a.x - p_b.x).abs() < EPS && (p_a.y - p_b.y).abs() < EPS {
        return None;
    }
    if side.is_horizontal() {
        // Left/Right: horizontal stems, vertical crossbar at x = cx. The crossbar
        // steps past the obstacles that overlap the degenerate straight line it
        // replaces — those in the y-band that ALSO straddle `base` (the shared
        // mount x). Obstacles off to the side (not on the straight) are irrelevant
        // and must NOT push the crossbar (that would drag the stems through them).
        let (lo, hi) = (p_a.y.min(p_b.y), p_a.y.max(p_b.y));
        let base = if side == Side::Right { p_a.x.max(p_b.x) } else { p_a.x.min(p_b.x) };
        let blocks = |r: &Rect| {
            r.y < hi + EPS && r.y + r.height > lo - EPS && r.x <= base + EPS && r.x + r.width >= base - EPS
        };
        let cx = if side == Side::Right {
            let mut m = base;
            for r in obstacles.iter().filter(|r| blocks(r)) {
                m = m.max(r.x + r.width);
            }
            m + stem
        } else {
            let mut m = base;
            for r in obstacles.iter().filter(|r| blocks(r)) {
                m = m.min(r.x);
            }
            m - stem
        };
        Some(vec![
            p_a.clone(),
            Point { x: cx, y: p_a.y },
            Point { x: cx, y: p_b.y },
            p_b.clone(),
        ])
    } else {
        // Top/Bottom: vertical stems, horizontal crossbar at y = cy. Same rule on
        // the other axis — step past only obstacles in the x-band that straddle
        // `base` (the shared mount y).
        let (lo, hi) = (p_a.x.min(p_b.x), p_a.x.max(p_b.x));
        let base = if side == Side::Bottom { p_a.y.max(p_b.y) } else { p_a.y.min(p_b.y) };
        let blocks = |r: &Rect| {
            r.x < hi + EPS && r.x + r.width > lo - EPS && r.y <= base + EPS && r.y + r.height >= base - EPS
        };
        let cy = if side == Side::Bottom {
            let mut m = base;
            for r in obstacles.iter().filter(|r| blocks(r)) {
                m = m.max(r.y + r.height);
            }
            m + stem
        } else {
            let mut m = base;
            for r in obstacles.iter().filter(|r| blocks(r)) {
                m = m.min(r.y);
            }
            m - stem
        };
        Some(vec![
            p_a.clone(),
            Point { x: p_a.x, y: cy },
            Point { x: p_b.x, y: cy },
            p_b.clone(),
        ])
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
        ); // C or Z — two bends
    }

    #[test]
    fn bend_score_is_about_shape_not_bend_count() {
        // straight 0, L 1
        assert_eq!(bend_score(&[p(0.0, 0.0), p(10.0, 0.0)]), 0.0);
        assert_eq!(bend_score(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)]), 1.0);

        // C (bracket "]"): right → down → left. Middle segment is the vertical at
        // x=10; BOTH ends (x=0 and x=3) sit LEFT of it → same side → C → 2.
        let c_bracket = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(3.0, 10.0)];
        assert_eq!(bends(&c_bracket), 2);
        assert!(!doubles_back(&c_bracket));
        assert_eq!(bend_score(&c_bracket), 2.0);

        // C (arch "∩"): up → over → down between two surfaces on the same plane.
        // Middle is the horizontal at y=80; BOTH ends (y=104) sit below → same side.
        let c_arch = [p(458.0, 104.0), p(458.0, 80.0), p(878.0, 80.0), p(878.0, 104.0)];
        assert_eq!(bend_score(&c_arch), 2.0);

        // Z (the facing-surface jog "_|ˉ"): right → down → right. SAME bend count as
        // a C (2), but the ends sit on OPPOSITE sides of the middle vertical (x=0
        // left, x=20 right) → Z → 99. This is the case bend-count alone gets wrong.
        let z_jog = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(20.0, 10.0)];
        assert_eq!(bends(&z_jog), 2);
        assert!(!doubles_back(&z_jog));
        assert_eq!(bend_score(&z_jog), Z_PENALTY);

        // Staircase: ≥3 bends → Z tier → 99.
        let staircase = [p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0), p(10.0, 5.0), p(10.0, 10.0)];
        assert_eq!(bends(&staircase), 3);
        assert_eq!(bend_score(&staircase), Z_PENALTY);

        // DOGLEG: the line doubles back OVER ITSELF — right to x=10 then back left
        // along the same y=0 line, overlapping itself → 1e9.
        let dogleg = [p(0.0, 0.0), p(10.0, 0.0), p(5.0, 0.0)];
        assert!(doubles_back(&dogleg));
        assert_eq!(bend_score(&dogleg), DOGLEG_PENALTY);
        assert!(DOGLEG_PENALTY > Z_PENALTY * 1_000_000.0);
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
