//! Screen-space pill placement: anchor-on-line + stagger + de-overlap.
//!
//! OWNERSHIP NOTE. The engine (`architext_routing::plan_diagram::place_label`)
//! emits a per-route label anchor (`route.label_x` / `route.label_y`). That
//! placement was designed for full-WORD text boxes — it de-conflicts using the
//! word's text-box dimensions against node rects and other word boxes. The
//! viewer, however, collapses each label to a *much smaller* glyph: a flow step
//! collapses to a radius-11 number badge, a structural relationship to an 18×18
//! kind pill. Because the rendered glyph is smaller and differently centered
//! than the box the engine de-conflicted, the engine anchor can land beside the
//! route or stack near a neighbour's anchor — the F1 (flow number pills bunch /
//! drift) and F9 (structural kind pills scatter / float) audit findings.
//!
//! This module does NOT touch the engine. It takes the engine anchor as a SEED
//! and re-derives a screen-space position the viewer can legitimately own:
//!   1. Anchor on the line  — project the seed onto the route polyline so the
//!      pill sits ON its own edge, never floating beside it.
//!   2. Stagger along length — when several pills land near the same point
//!      (parallel/close lines, or several on one edge), slide each along its own
//!      polyline so they separate.
//!   3. De-overlap          — any pill still too close to an already-placed pill
//!      is nudged: first further along its own line, and only as a last resort
//!      perpendicular to it.
//!
//! It is a pure function of geometry (no DOM, no Leptos) so the math is unit
//! tested directly.

use architext_routing::model::Point;

/// Collision radius (canvas px) used to decide two pills are "too close". The
/// flow number badge has radius 11 and the structural pill is 18×18 (half-extent
/// 9); a 24px center separation clears either with a small breathing gap and
/// matches the engine's own label clearance scale.
const MIN_SEPARATION: f64 = 24.0;

/// How far along its own route a pill may slide (canvas px of arc length, in
/// each direction from the anchor) while searching for a non-colliding spot
/// before falling back to a perpendicular nudge. Bounded so a pill never slides
/// off onto the wrong end of a long edge.
const MAX_SLIDE: f64 = 60.0;

/// Arc-length step when searching along a route for a free slot.
const SLIDE_STEP: f64 = 8.0;

/// Perpendicular nudge distance used only when sliding along the line cannot
/// clear the collision (e.g. two short parallel edges fully overlapping).
const PERP_NUDGE: f64 = 18.0;

/// One pill's placement request: the route polyline it belongs to and the
/// engine's seed anchor for it.
#[derive(Clone, Debug)]
pub struct PillInput {
    /// The route polyline (engine `route.points`). May be empty (degenerate
    /// route) — then the seed is used verbatim.
    pub route_points: Vec<Point>,
    /// Engine seed anchor (`route.label_x` / `route.label_y`).
    pub seed: (f64, f64),
}

/// Resolve final pill anchors for a set of pills. The returned `(x, y)` vector
/// is parallel to `inputs`. Placement is deterministic and order-stable: pills
/// are placed in input order, each de-conflicted against those already placed.
pub fn place_pills(inputs: &[PillInput]) -> Vec<(f64, f64)> {
    let mut placed: Vec<(f64, f64)> = Vec::with_capacity(inputs.len());
    for input in inputs {
        // 1. Anchor on the line (or use the seed if the route is degenerate).
        let anchored = anchor_on_route(&input.route_points, input.seed);
        // 2 + 3. Slide along the line, then perpendicular, to clear collisions.
        let final_pos = resolve_collision(anchored, &input.route_points, &placed);
        placed.push(final_pos);
    }
    placed
}

/// Project `seed` onto the nearest point of the polyline `points`, returning
/// that on-line point. With <2 points there is no line to anchor to, so the seed
/// is returned unchanged.
fn anchor_on_route(points: &[Point], seed: (f64, f64)) -> (f64, f64) {
    if points.len() < 2 {
        return seed;
    }
    let mut best = (points[0].x, points[0].y);
    let mut best_d2 = f64::INFINITY;
    for seg in points.windows(2) {
        let (px, py) = nearest_on_segment(seg[0].x, seg[0].y, seg[1].x, seg[1].y, seed.0, seed.1);
        let d2 = dist2(px, py, seed.0, seed.1);
        if d2 < best_d2 {
            best_d2 = d2;
            best = (px, py);
        }
    }
    best
}

/// Find a position for an anchored pill that clears every already-`placed` pill
/// by `MIN_SEPARATION`. Strategy: keep the anchor if already clear; else slide
/// along the route in both directions in `SLIDE_STEP` increments up to
/// `MAX_SLIDE`; else nudge perpendicular to the local route direction.
fn resolve_collision(
    anchor: (f64, f64),
    points: &[Point],
    placed: &[(f64, f64)],
) -> (f64, f64) {
    if !collides(anchor, placed) {
        return anchor;
    }

    // Slide along the line. We parameterize by arc length from the anchor's
    // projection and probe outward symmetrically (nearer slots first) so pills
    // fan out evenly rather than all drifting one way.
    if points.len() >= 2 {
        let anchor_t = arc_len_at_point(points, anchor);
        let total = total_arc_len(points);
        let mut offset = SLIDE_STEP;
        while offset <= MAX_SLIDE {
            for signed in [offset, -offset] {
                let t = anchor_t + signed;
                if t < 0.0 || t > total {
                    continue;
                }
                let cand = point_at_arc_len(points, t);
                if !collides(cand, placed) {
                    return cand;
                }
            }
            offset += SLIDE_STEP;
        }
    }

    // Last resort: nudge perpendicular to the local route direction. Try both
    // sides; pick the first clear one, else the side that maximizes clearance.
    let (nx, ny) = perpendicular(points, anchor);
    let plus = (anchor.0 + nx * PERP_NUDGE, anchor.1 + ny * PERP_NUDGE);
    if !collides(plus, placed) {
        return plus;
    }
    let minus = (anchor.0 - nx * PERP_NUDGE, anchor.1 - ny * PERP_NUDGE);
    if !collides(minus, placed) {
        return minus;
    }
    // Both perpendicular sides still collide: keep the one with more clearance
    // so we at least move away from the densest cluster.
    if min_clearance(plus, placed) >= min_clearance(minus, placed) {
        plus
    } else {
        minus
    }
}

/// True if `p` is within `MIN_SEPARATION` of any already-placed pill.
fn collides(p: (f64, f64), placed: &[(f64, f64)]) -> bool {
    let min_sep2 = MIN_SEPARATION * MIN_SEPARATION;
    placed.iter().any(|q| dist2(p.0, p.1, q.0, q.1) < min_sep2)
}

/// Distance from `p` to its nearest placed pill (∞ if none placed).
fn min_clearance(p: (f64, f64), placed: &[(f64, f64)]) -> f64 {
    placed
        .iter()
        .map(|q| dist2(p.0, p.1, q.0, q.1))
        .fold(f64::INFINITY, f64::min)
}

/// Nearest point to `(px, py)` on the segment `a→b`.
fn nearest_on_segment(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> (f64, f64) {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::EPSILON {
        return (ax, ay);
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    (ax + t * dx, ay + t * dy)
}

/// Total arc length of the polyline.
fn total_arc_len(points: &[Point]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
        .sum()
}

/// Arc length from the polyline start to the projection of `p` onto the line.
fn arc_len_at_point(points: &[Point], p: (f64, f64)) -> f64 {
    let mut acc = 0.0;
    let mut best_len = 0.0;
    let mut best_d2 = f64::INFINITY;
    for seg in points.windows(2) {
        let (ax, ay) = (seg[0].x, seg[0].y);
        let (bx, by) = (seg[1].x, seg[1].y);
        let seg_len = (bx - ax).hypot(by - ay);
        let (nx, ny) = nearest_on_segment(ax, ay, bx, by, p.0, p.1);
        let d2 = dist2(nx, ny, p.0, p.1);
        if d2 < best_d2 {
            best_d2 = d2;
            best_len = acc + (nx - ax).hypot(ny - ay);
        }
        acc += seg_len;
    }
    best_len
}

/// The point at arc length `target` along the polyline (clamped to its ends).
fn point_at_arc_len(points: &[Point], target: f64) -> (f64, f64) {
    if points.is_empty() {
        return (0.0, 0.0);
    }
    if target <= 0.0 {
        return (points[0].x, points[0].y);
    }
    let mut acc = 0.0;
    for seg in points.windows(2) {
        let (ax, ay) = (seg[0].x, seg[0].y);
        let (bx, by) = (seg[1].x, seg[1].y);
        let seg_len = (bx - ax).hypot(by - ay);
        if acc + seg_len >= target {
            let t = if seg_len <= f64::EPSILON { 0.0 } else { (target - acc) / seg_len };
            return (ax + t * (bx - ax), ay + t * (by - ay));
        }
        acc += seg_len;
    }
    let last = &points[points.len() - 1];
    (last.x, last.y)
}

/// Unit perpendicular to the local route direction near `p` (defaults to a
/// vertical nudge if direction is undefined).
fn perpendicular(points: &[Point], p: (f64, f64)) -> (f64, f64) {
    if points.len() < 2 {
        return (0.0, 1.0);
    }
    // Find the segment nearest p and use its direction.
    let mut best_dir = (0.0_f64, 0.0_f64);
    let mut best_d2 = f64::INFINITY;
    for seg in points.windows(2) {
        let (ax, ay) = (seg[0].x, seg[0].y);
        let (bx, by) = (seg[1].x, seg[1].y);
        let (nx, ny) = nearest_on_segment(ax, ay, bx, by, p.0, p.1);
        let d2 = dist2(nx, ny, p.0, p.1);
        if d2 < best_d2 {
            best_d2 = d2;
            best_dir = (bx - ax, by - ay);
        }
    }
    let len = best_dir.0.hypot(best_dir.1);
    if len <= f64::EPSILON {
        return (0.0, 1.0);
    }
    // Rotate the unit direction 90°: (dx, dy) -> (-dy, dx).
    (-best_dir.1 / len, best_dir.0 / len)
}

fn dist2(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    /// A seed floating beside its line is pulled ONTO the line.
    #[test]
    fn anchor_pulls_seed_onto_the_route() {
        // Horizontal line y=0 from x=0..100; seed floats 20px above the midpoint.
        let line = vec![pt(0.0, 0.0), pt(100.0, 0.0)];
        let (x, y) = anchor_on_route(&line, (50.0, 20.0));
        assert!((x - 50.0).abs() < 1e-9, "x stays at the nearest-point x");
        assert!((y - 0.0).abs() < 1e-9, "y is pulled onto the line");
    }

    /// A degenerate (single-point / empty) route keeps the seed verbatim.
    #[test]
    fn degenerate_route_keeps_the_seed() {
        assert_eq!(anchor_on_route(&[], (7.0, 9.0)), (7.0, 9.0));
        assert_eq!(anchor_on_route(&[pt(1.0, 2.0)], (7.0, 9.0)), (7.0, 9.0));
    }

    /// Two pills seeded at the SAME point on two parallel close lines must end
    /// up separated by at least the minimum separation.
    #[test]
    fn coincident_pills_are_staggered_apart() {
        // Two horizontal lines, both long, near each other. Both seeds collapse
        // to the same (50, 0) point.
        let a = PillInput { route_points: vec![pt(0.0, 0.0), pt(100.0, 0.0)], seed: (50.0, 0.0) };
        let b = PillInput { route_points: vec![pt(0.0, 5.0), pt(100.0, 5.0)], seed: (50.0, 0.0) };
        let out = place_pills(&[a, b]);
        let d = (out[0].0 - out[1].0).hypot(out[0].1 - out[1].1);
        assert!(d >= MIN_SEPARATION - 1e-6, "pills must be ≥ MIN_SEPARATION apart, got {d}");
    }

    /// Placed pills always sit ON their own route after staggering (the slide is
    /// along arc length, so the result stays on the polyline).
    #[test]
    fn staggered_pills_stay_on_their_line() {
        let a = PillInput { route_points: vec![pt(0.0, 0.0), pt(100.0, 0.0)], seed: (50.0, 0.0) };
        let b = PillInput { route_points: vec![pt(0.0, 0.0), pt(100.0, 0.0)], seed: (50.0, 0.0) };
        let c = PillInput { route_points: vec![pt(0.0, 0.0), pt(100.0, 0.0)], seed: (50.0, 0.0) };
        let out = place_pills(&[a, b, c]);
        // All three share the same horizontal line y=0; each result must lie on it.
        for (_, y) in &out {
            assert!(y.abs() < 1e-6, "pill slid along its line stays at y=0, got {y}");
        }
        // And all three are mutually separated.
        for i in 0..out.len() {
            for j in (i + 1)..out.len() {
                let d = (out[i].0 - out[j].0).hypot(out[i].1 - out[j].1);
                assert!(d >= MIN_SEPARATION - 1e-6, "pair {i},{j} too close: {d}");
            }
        }
    }

    /// Non-colliding pills are left exactly on their anchor (no needless nudging).
    #[test]
    fn isolated_pill_keeps_its_anchor() {
        let a = PillInput { route_points: vec![pt(0.0, 0.0), pt(100.0, 0.0)], seed: (50.0, 0.0) };
        let b = PillInput { route_points: vec![pt(0.0, 200.0), pt(100.0, 200.0)], seed: (50.0, 200.0) };
        let out = place_pills(&[a, b]);
        assert_eq!(out[0], (50.0, 0.0));
        assert_eq!(out[1], (50.0, 200.0));
    }

    /// When the lines are too short to slide far enough, a perpendicular nudge
    /// still separates the pills.
    #[test]
    fn fully_overlapping_short_lines_fall_back_to_perpendicular() {
        // Two identical very short segments — sliding can't create separation,
        // so the second pill must be nudged perpendicular.
        let a = PillInput { route_points: vec![pt(0.0, 0.0), pt(4.0, 0.0)], seed: (2.0, 0.0) };
        let b = PillInput { route_points: vec![pt(0.0, 0.0), pt(4.0, 0.0)], seed: (2.0, 0.0) };
        let out = place_pills(&[a, b]);
        let d = (out[0].0 - out[1].0).hypot(out[0].1 - out[1].1);
        assert!(d >= PERP_NUDGE - 1e-6, "perpendicular nudge separates them, got {d}");
    }

    /// `point_at_arc_len` walks the polyline correctly across segment joints.
    #[test]
    fn point_at_arc_len_walks_segments() {
        // L-shaped path: (0,0)->(10,0)->(10,10). Total length 20.
        let path = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        assert_eq!(point_at_arc_len(&path, 0.0), (0.0, 0.0));
        assert_eq!(point_at_arc_len(&path, 5.0), (5.0, 0.0));
        assert_eq!(point_at_arc_len(&path, 10.0), (10.0, 0.0));
        assert_eq!(point_at_arc_len(&path, 15.0), (10.0, 5.0));
        assert_eq!(point_at_arc_len(&path, 100.0), (10.0, 10.0)); // clamped
    }
}
