//! Faithful port of `viewer/src/routing/routeGeometry.js`.
//!
//! Every function is translated 1:1. Geometry math is parity-critical: the
//! routing fingerprint harness (Phase 1B) gates byte-identical output. Key
//! decisions:
//!
//! - `Math.hypot(a, b)` / `Math.sqrt(a*a+b*b)` → `js_compat::js_hypot`
//!   (libm::hypot, bit-identical on native + wasm32).
//! - `Math.round` → `js_compat::js_round` (JS half-toward-+∞ semantics).
//! - `Math.floor`, `Math.ceil`, `Math.abs`, `Math.min`, `Math.max` →
//!   `f64::{floor,ceil,abs,min,max}` (agree with JS for finite values).
//! - `dedupeBy` from routeConstants.js is re-used from `crate::route_constants`.
//! - Point shapes match `crate::model::Point` (`{x, y}`); Rect matches
//!   `crate::model::Rect` (`{x, y, width, height}`).

use crate::js_compat::{js_hypot, js_round};
use crate::model::{Point, Rect};
use crate::route_constants::dedupe_by;

// ---------------------------------------------------------------------------
// clamp
// ---------------------------------------------------------------------------

/// Port of JS `clamp(value, min, max)`.
///
/// `Math.min(max, Math.max(min, value))`
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    f64::min(max, f64::max(min, value))
}

// ---------------------------------------------------------------------------
// distanceToRect / distanceToRectSquared
// ---------------------------------------------------------------------------

/// Port of JS `distanceToRect(point, rect)`.
///
/// Returns 0 when the point is inside or on the boundary of the rect.
pub fn distance_to_rect(point: &Point, rect: &Rect) -> f64 {
    let dx = f64::max(rect.x - point.x, f64::max(0.0, point.x - (rect.x + rect.width)));
    let dy = f64::max(rect.y - point.y, f64::max(0.0, point.y - (rect.y + rect.height)));
    js_hypot(dx, dy)
}

/// Port of JS `distanceToRectSquared(point, rect)`.
///
/// Squared variant — avoids hypot; used for relative comparisons.
pub fn distance_to_rect_squared(point: &Point, rect: &Rect) -> f64 {
    let dx = f64::max(rect.x - point.x, f64::max(0.0, point.x - (rect.x + rect.width)));
    let dy = f64::max(rect.y - point.y, f64::max(0.0, point.y - (rect.y + rect.height)));
    dx * dx + dy * dy
}

// ---------------------------------------------------------------------------
// unitVector
// ---------------------------------------------------------------------------

/// Port of JS `unitVector(from, to)`.
///
/// Returns `{x: 1, y: 0}` when `from === to` (zero-length vector), matching
/// the JS fallback `if (length === 0) return { x: 1, y: 0 }`.
pub fn unit_vector(from: &Point, to: &Point) -> Point {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length = js_hypot(dx, dy);
    if length == 0.0 {
        return Point { x: 1.0, y: 0.0 };
    }
    Point { x: dx / length, y: dy / length }
}

// ---------------------------------------------------------------------------
// lineSamples / sampleLine
// ---------------------------------------------------------------------------

/// Port of JS `lineSamples(points)`.
///
/// For each consecutive pair of points, produces 10 evenly-spaced samples
/// at t = 0.1, 0.2, …, 1.0.  A single point returns an empty vec.
pub fn line_samples(points: &[Point]) -> Vec<Point> {
    let mut samples = Vec::new();
    for index in 0..points.len().saturating_sub(1) {
        let start = &points[index];
        let end = &points[index + 1];
        for step in 1..=10_u32 {
            let t = step as f64 / 10.0;
            samples.push(Point {
                x: start.x + (end.x - start.x) * t,
                y: start.y + (end.y - start.y) * t,
            });
        }
    }
    samples
}

/// Port of JS `sampleLine(start, end, steps = 10)`.
///
/// Produces `steps` evenly-spaced samples at t = 1/steps, 2/steps, …, 1.0.
pub fn sample_line(start: &Point, end: &Point, steps: u32) -> Vec<Point> {
    let mut samples = Vec::new();
    for step in 1..=steps {
        let t = step as f64 / steps as f64;
        samples.push(Point {
            x: start.x + (end.x - start.x) * t,
            y: start.y + (end.y - start.y) * t,
        });
    }
    samples
}

// ---------------------------------------------------------------------------
// cubicPoint / sampleCubic
// ---------------------------------------------------------------------------

/// Port of JS `cubicPoint(start, controlA, controlB, end, t)`.
///
/// Evaluates the cubic Bézier at parameter `t` ∈ [0, 1].
pub fn cubic_point(start: &Point, control_a: &Point, control_b: &Point, end: &Point, t: f64) -> Point {
    let inverse = 1.0 - t;
    Point {
        x: inverse.powi(3) * start.x
            + 3.0 * inverse.powi(2) * t * control_a.x
            + 3.0 * inverse * t.powi(2) * control_b.x
            + t.powi(3) * end.x,
        y: inverse.powi(3) * start.y
            + 3.0 * inverse.powi(2) * t * control_a.y
            + 3.0 * inverse * t.powi(2) * control_b.y
            + t.powi(3) * end.y,
    }
}

/// Port of JS `sampleCubic(start, controlA, controlB, end, steps = 16)`.
///
/// Produces `steps` samples at t = 1/steps, 2/steps, …, 1.0.
pub fn sample_cubic(
    start: &Point,
    control_a: &Point,
    control_b: &Point,
    end: &Point,
    steps: u32,
) -> Vec<Point> {
    let mut samples = Vec::new();
    for step in 1..=steps {
        samples.push(cubic_point(start, control_a, control_b, end, step as f64 / steps as f64));
    }
    samples
}

// ---------------------------------------------------------------------------
// nearestSample
// ---------------------------------------------------------------------------

/// Port of JS `nearestSample(samples, target)`.
///
/// Returns the sample closest (Euclidean) to `target`.  On an equal-distance
/// tie the first sample wins (JS `reduce` does not replace on equal). When
/// `samples` is empty, returns `target` (JS `samples[0] ?? target`).
pub fn nearest_sample<'a>(samples: &'a [Point], target: &'a Point) -> &'a Point {
    if samples.is_empty() {
        return target;
    }
    let mut nearest = &samples[0];
    let mut nearest_dist = js_hypot(nearest.x - target.x, nearest.y - target.y);
    for sample in &samples[1..] {
        let d = js_hypot(sample.x - target.x, sample.y - target.y);
        // strict less-than: ties keep the first (earlier) sample, matching JS reduce
        if d < nearest_dist {
            nearest = sample;
            nearest_dist = d;
        }
    }
    nearest
}

// ---------------------------------------------------------------------------
// rectDistance / rectsOverlap
// ---------------------------------------------------------------------------

/// Port of JS `rectDistance(a, b)`.
///
/// Returns 0 when rects overlap or touch.
pub fn rect_distance(a: &Rect, b: &Rect) -> f64 {
    let dx = f64::max(a.x - (b.x + b.width), f64::max(b.x - (a.x + a.width), 0.0));
    let dy = f64::max(a.y - (b.y + b.height), f64::max(b.y - (a.y + a.height), 0.0));
    js_hypot(dx, dy)
}

/// Port of JS `rectsOverlap(a, b, padding = 0)`.
///
/// `padding` inflates `b` (each side) before testing overlap. Boundaries
/// touching (strict inequality fails) are **not** considered overlapping.
pub fn rects_overlap(a: &Rect, b: &Rect, padding: f64) -> bool {
    a.x < b.x + b.width + padding
        && a.x + a.width > b.x - padding
        && a.y < b.y + b.height + padding
        && a.y + a.height > b.y - padding
}

// ---------------------------------------------------------------------------
// boundsForPoints
// ---------------------------------------------------------------------------

/// Port of JS `boundsForPoints(points)`.
///
/// Returns `{x:0, y:0, width:0, height:0}` for an empty slice.
pub fn bounds_for_points(points: &[Point]) -> Rect {
    if points.is_empty() {
        return Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
    }
    let mut min_x = points[0].x;
    let mut max_x = points[0].x;
    let mut min_y = points[0].y;
    let mut max_y = points[0].y;
    for point in &points[1..] {
        min_x = f64::min(min_x, point.x);
        max_x = f64::max(max_x, point.x);
        min_y = f64::min(min_y, point.y);
        max_y = f64::max(max_y, point.y);
    }
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

// ---------------------------------------------------------------------------
// segmentIntersectsRect
// ---------------------------------------------------------------------------

/// Port of JS `segmentIntersectsRect(start, end, rect, padding = 0)`.
///
/// Only handles horizontal and vertical segments (returns `false` for
/// diagonal segments), mirroring the JS source exactly.
pub fn segment_intersects_rect(start: &Point, end: &Point, rect: &Rect, padding: f64) -> bool {
    let left = rect.x - padding;
    let right = rect.x + rect.width + padding;
    let top = rect.y - padding;
    let bottom = rect.y + rect.height + padding;
    let min_x = f64::min(start.x, end.x);
    let max_x = f64::max(start.x, end.x);
    let min_y = f64::min(start.y, end.y);
    let max_y = f64::max(start.y, end.y);

    if start.y == end.y {
        return start.y > top && start.y < bottom && max_x > left && min_x < right;
    }
    if start.x == end.x {
        return start.x > left && start.x < right && max_y > top && min_y < bottom;
    }
    false
}

// ---------------------------------------------------------------------------
// bendCount
// ---------------------------------------------------------------------------

/// Port of JS `bendCount(points)`.
///
/// Counts direction changes: a bend is a point where the incoming axis
/// disagrees with the outgoing axis (horizontal-then-vertical or vice-versa).
pub fn bend_count(points: &[Point]) -> usize {
    let mut bends = 0usize;
    for index in 1..points.len().saturating_sub(1) {
        let previous = &points[index - 1];
        let current = &points[index];
        let next = &points[index + 1];
        if (previous.x == current.x && current.x != next.x)
            || (previous.y == current.y && current.y != next.y)
        {
            bends += 1;
        }
    }
    bends
}

// ---------------------------------------------------------------------------
// shallowJogCount
// ---------------------------------------------------------------------------

/// Port of JS `shallowJogCount(points)`.
///
/// A shallow jog is a short (< 36 px) perpendicular segment between two
/// parallel segments: a horizontal-then-vertical-then-horizontal Z or an
/// equivalent vertical variant.
pub fn shallow_jog_count(points: &[Point]) -> usize {
    let mut count = 0usize;
    for index in 1..points.len().saturating_sub(2) {
        let before = &points[index - 1];
        let start = &points[index];
        let end = &points[index + 1];
        let after = &points[index + 2];
        let middle_length = js_hypot(end.x - start.x, end.y - start.y);
        let horizontal_jog = before.y == start.y && end.y == after.y && start.x == end.x;
        let vertical_jog = before.x == start.x && end.x == after.x && start.y == end.y;
        if (horizontal_jog || vertical_jog) && middle_length < 36.0 {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// routeLength
// ---------------------------------------------------------------------------

/// Port of JS `routeLength(samples)`.
///
/// Sums the Euclidean length of each segment in the polyline.
pub fn route_length(samples: &[Point]) -> f64 {
    let mut length = 0.0_f64;
    for index in 0..samples.len().saturating_sub(1) {
        length += js_hypot(
            samples[index + 1].x - samples[index].x,
            samples[index + 1].y - samples[index].y,
        );
    }
    length
}

// ---------------------------------------------------------------------------
// pointAtDistance
// ---------------------------------------------------------------------------

/// Port of JS `pointAtDistance(samples, distance)`.
///
/// Walks `samples` and returns the interpolated point at the given arc
/// distance from the start. Returns `None` for empty input (JS `null`).
/// If `distance` exceeds total length, returns the last sample.
pub fn point_at_distance(samples: &[Point], distance: f64) -> Option<Point> {
    if samples.is_empty() {
        return None;
    }
    let mut traveled = 0.0_f64;
    for index in 0..samples.len() - 1 {
        let start = &samples[index];
        let end = &samples[index + 1];
        let segment_length = js_hypot(end.x - start.x, end.y - start.y);
        if traveled + segment_length >= distance {
            let t = if segment_length == 0.0 {
                0.0
            } else {
                (distance - traveled) / segment_length
            };
            return Some(Point {
                x: start.x + (end.x - start.x) * t,
                y: start.y + (end.y - start.y) * t,
            });
        }
        traveled += segment_length;
    }
    // distance exceeded total length: return last sample
    Some(samples[samples.len() - 1].clone())
}

// ---------------------------------------------------------------------------
// uniqueRounded
// ---------------------------------------------------------------------------

/// Port of JS `uniqueRounded(values)`.
///
/// Rounds each value with `js_round` then deduplicates (first-occurrence wins).
/// JS `-0` and `0` are the same Set key, so both round to 0 and deduplicate.
/// We reproduce this by using the raw f64 bits for the set key but treating
/// -0.0 and 0.0 as equal (they compare == in f64).
pub fn unique_rounded(values: &[f64]) -> Vec<f64> {
    let rounded: Vec<f64> = values.iter().map(|&v| js_round(v)).collect();
    // dedupeBy uses the value itself as key; -0.0 == 0.0 in f64 equality, but
    // they have different bit patterns. JS Set uses SameValueZero which treats
    // -0 === 0. We canonicalize -0.0 → 0.0 to match that behaviour.
    dedupe_by(rounded, |v| {
        let canonical = if *v == 0.0 { 0.0_f64 } else { *v };
        canonical.to_bits()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    // ----- clamp -----

    #[test]
    fn clamp_within_range() {
        assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
    }

    #[test]
    fn clamp_below_min() {
        assert_eq!(clamp(-5.0, 0.0, 10.0), 0.0);
    }

    #[test]
    fn clamp_above_max() {
        assert_eq!(clamp(15.0, 0.0, 10.0), 10.0);
    }

    #[test]
    fn clamp_at_boundary() {
        assert_eq!(clamp(0.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp(10.0, 0.0, 10.0), 10.0);
    }

    // ----- distanceToRect -----

    #[test]
    fn distance_to_rect_inside_is_zero() {
        // Point inside rect → 0
        assert_eq!(distance_to_rect(&pt(50.0, 50.0), &rect(0.0, 0.0, 100.0, 100.0)), 0.0);
    }

    #[test]
    fn distance_to_rect_right_of_rect() {
        // Point 10px to the right of the right edge
        assert_eq!(distance_to_rect(&pt(110.0, 50.0), &rect(0.0, 0.0, 100.0, 100.0)), 10.0);
    }

    #[test]
    fn distance_to_rect_above_right_corner() {
        // Point 3px right and 4px above top-right corner → 5
        assert_eq!(distance_to_rect(&pt(103.0, -4.0), &rect(0.0, 0.0, 100.0, 100.0)), 5.0);
    }

    #[test]
    fn distance_to_rect_below_left_corner() {
        // Point 3px left and 4px below bottom-left corner → 5
        assert_eq!(distance_to_rect(&pt(-3.0, 104.0), &rect(0.0, 0.0, 100.0, 100.0)), 5.0);
    }

    // ----- distanceToRectSquared -----

    #[test]
    fn distance_to_rect_squared_inside_is_zero() {
        assert_eq!(distance_to_rect_squared(&pt(50.0, 50.0), &rect(0.0, 0.0, 100.0, 100.0)), 0.0);
    }

    #[test]
    fn distance_to_rect_squared_right_of_rect() {
        assert_eq!(distance_to_rect_squared(&pt(110.0, 50.0), &rect(0.0, 0.0, 100.0, 100.0)), 100.0);
    }

    #[test]
    fn distance_to_rect_squared_corner() {
        // (3,4) corner → 9+16=25
        assert_eq!(distance_to_rect_squared(&pt(103.0, -4.0), &rect(0.0, 0.0, 100.0, 100.0)), 25.0);
    }

    // ----- unitVector -----

    #[test]
    fn unit_vector_3_4() {
        let u = unit_vector(&pt(0.0, 0.0), &pt(3.0, 4.0));
        assert_eq!(u.x, 0.6);
        assert_eq!(u.y, 0.8);
    }

    #[test]
    fn unit_vector_zero_length_returns_x1_y0() {
        let u = unit_vector(&pt(0.0, 0.0), &pt(0.0, 0.0));
        assert_eq!(u.x, 1.0);
        assert_eq!(u.y, 0.0);
    }

    #[test]
    fn unit_vector_pure_y_axis() {
        let u = unit_vector(&pt(10.0, 10.0), &pt(10.0, 20.0));
        assert_eq!(u.x, 0.0);
        assert_eq!(u.y, 1.0);
    }

    // ----- lineSamples -----

    #[test]
    fn line_samples_single_segment() {
        // one segment [0,0]→[10,0]: 10 samples at x=1..10
        let pts = vec![pt(0.0, 0.0), pt(10.0, 0.0)];
        let s = line_samples(&pts);
        assert_eq!(s.len(), 10);
        assert_eq!(s[0], pt(1.0, 0.0));
        assert_eq!(s[9], pt(10.0, 0.0));
    }

    #[test]
    fn line_samples_two_segments() {
        // [0,0]→[10,0]→[10,10]: 20 samples; segment junction at index 9/10
        let pts = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        let s = line_samples(&pts);
        assert_eq!(s.len(), 20);
        assert_eq!(s[9], pt(10.0, 0.0));   // end of first segment
        assert_eq!(s[10], pt(10.0, 1.0));  // start of second segment (t=0.1)
        assert_eq!(s[19], pt(10.0, 10.0)); // end of second segment
    }

    #[test]
    fn line_samples_single_point_is_empty() {
        let pts = vec![pt(5.0, 5.0)];
        assert!(line_samples(&pts).is_empty());
    }

    // ----- sampleLine -----

    #[test]
    fn sample_line_five_steps() {
        // [0,0]→[10,0] with 5 steps: t = 0.2, 0.4, 0.6, 0.8, 1.0
        let s = sample_line(&pt(0.0, 0.0), &pt(10.0, 0.0), 5);
        assert_eq!(s.len(), 5);
        assert_eq!(s[0], pt(2.0, 0.0));
        assert_eq!(s[4], pt(10.0, 0.0));
    }

    // ----- cubicPoint -----

    #[test]
    fn cubic_point_t0_returns_start() {
        let p = cubic_point(&pt(0.0, 0.0), &pt(0.0, 100.0), &pt(100.0, 100.0), &pt(100.0, 0.0), 0.0);
        assert_eq!(p, pt(0.0, 0.0));
    }

    #[test]
    fn cubic_point_t1_returns_end() {
        let p = cubic_point(&pt(0.0, 0.0), &pt(0.0, 100.0), &pt(100.0, 100.0), &pt(100.0, 0.0), 1.0);
        assert_eq!(p, pt(100.0, 0.0));
    }

    #[test]
    fn cubic_point_t_half_symmetric() {
        // Symmetric S-curve: at t=0.5 the y-component is 75 (not 50) due to the S shape
        let p = cubic_point(&pt(0.0, 0.0), &pt(0.0, 100.0), &pt(100.0, 100.0), &pt(100.0, 0.0), 0.5);
        assert_eq!(p, pt(50.0, 75.0));
    }

    #[test]
    fn cubic_point_t_quarter() {
        // t=0.25 on [0,0]->[50,0]->[100,0]->[100,100]
        // Expected from Node: {"x":36.71875,"y":1.5625}
        let p = cubic_point(&pt(0.0, 0.0), &pt(50.0, 0.0), &pt(100.0, 0.0), &pt(100.0, 100.0), 0.25);
        assert_eq!(p.x, 36.71875);
        assert_eq!(p.y, 1.5625);
    }

    // ----- sampleCubic -----

    #[test]
    fn sample_cubic_default_16_steps() {
        let s = sample_cubic(
            &pt(0.0, 0.0), &pt(0.0, 100.0), &pt(100.0, 100.0), &pt(100.0, 0.0), 16
        );
        assert_eq!(s.len(), 16);
        // Step 1 (t=1/16): from Node golden {x:1.123046875, y:17.578125}
        assert_eq!(s[0].x, 1.123046875);
        assert_eq!(s[0].y, 17.578125);
        // Step 8 (t=0.5): symmetric midpoint {x:50, y:75}
        assert_eq!(s[7], pt(50.0, 75.0));
        // Step 16 (t=1.0): end point {x:100, y:0}
        assert_eq!(s[15], pt(100.0, 0.0));
    }

    #[test]
    fn sample_cubic_4_steps() {
        // From Node: [{x:29.6875,y:15.625},{x:50,y:50},{x:70.3125,y:84.375},{x:100,y:100}]
        let s = sample_cubic(
            &pt(0.0, 0.0), &pt(50.0, 0.0), &pt(50.0, 100.0), &pt(100.0, 100.0), 4
        );
        assert_eq!(s.len(), 4);
        assert_eq!(s[0], pt(29.6875, 15.625));
        assert_eq!(s[1], pt(50.0, 50.0));
        assert_eq!(s[3], pt(100.0, 100.0));
    }

    // ----- nearestSample -----

    #[test]
    fn nearest_sample_selects_closest() {
        let samples = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)];
        assert_eq!(nearest_sample(&samples, &pt(12.0, 0.0)), &pt(10.0, 0.0));
        assert_eq!(nearest_sample(&samples, &pt(18.0, 0.0)), &pt(20.0, 0.0));
    }

    #[test]
    fn nearest_sample_tie_prefers_first() {
        // target at x=1, equidistant from x=0 and x=2 → first wins (JS reduce does not replace on equal)
        let samples = vec![pt(0.0, 0.0), pt(2.0, 0.0)];
        assert_eq!(nearest_sample(&samples, &pt(1.0, 0.0)), &pt(0.0, 0.0));
    }

    #[test]
    fn nearest_sample_empty_returns_target() {
        let target = pt(5.0, 5.0);
        assert_eq!(nearest_sample(&[], &target), &target);
    }

    // ----- rectDistance -----

    #[test]
    fn rect_distance_adjacent_is_zero() {
        // touching rects (share an edge) → 0
        assert_eq!(rect_distance(&rect(0.0, 0.0, 10.0, 10.0), &rect(10.0, 0.0, 10.0, 10.0)), 0.0);
    }

    #[test]
    fn rect_distance_overlapping_is_zero() {
        assert_eq!(rect_distance(&rect(0.0, 0.0, 20.0, 20.0), &rect(10.0, 10.0, 20.0, 20.0)), 0.0);
    }

    #[test]
    fn rect_distance_separated_h() {
        // 5px horizontal gap
        assert_eq!(rect_distance(&rect(0.0, 0.0, 10.0, 10.0), &rect(15.0, 0.0, 10.0, 10.0)), 5.0);
    }

    #[test]
    fn rect_distance_separated_corner() {
        // corner: dx=3, dy=4 → 5
        assert_eq!(rect_distance(&rect(0.0, 0.0, 10.0, 10.0), &rect(13.0, 14.0, 10.0, 10.0)), 5.0);
    }

    // ----- rectsOverlap -----

    #[test]
    fn rects_overlap_true_for_overlapping() {
        assert!(rects_overlap(
            &rect(0.0, 0.0, 10.0, 10.0),
            &rect(5.0, 5.0, 10.0, 10.0),
            0.0
        ));
    }

    #[test]
    fn rects_overlap_false_for_touching() {
        // Touching boundary (not strictly inside) → false
        assert!(!rects_overlap(
            &rect(0.0, 0.0, 10.0, 10.0),
            &rect(10.0, 0.0, 10.0, 10.0),
            0.0
        ));
    }

    #[test]
    fn rects_overlap_padding_makes_them_overlap() {
        // gap = 2 between {0..10} and {12..22}; padding 3 → b.x-padding = 9, 10 > 9 true
        assert!(rects_overlap(
            &rect(0.0, 0.0, 10.0, 10.0),
            &rect(12.0, 0.0, 10.0, 10.0),
            3.0
        ));
    }

    #[test]
    fn rects_overlap_padding_insufficient() {
        // gap = 5 between {0..10} and {15..25}; padding 4 → b.x-padding = 11, 10 > 11 false
        assert!(!rects_overlap(
            &rect(0.0, 0.0, 10.0, 10.0),
            &rect(15.0, 0.0, 10.0, 10.0),
            4.0
        ));
    }

    // ----- boundsForPoints -----

    #[test]
    fn bounds_for_points_empty() {
        assert_eq!(bounds_for_points(&[]), rect(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn bounds_for_points_single_point() {
        assert_eq!(bounds_for_points(&[pt(5.0, 3.0)]), rect(5.0, 3.0, 0.0, 0.0));
    }

    #[test]
    fn bounds_for_points_three_points() {
        // Node: {x:0, y:5, width:20, height:25}
        let pts = vec![pt(0.0, 10.0), pt(20.0, 5.0), pt(10.0, 30.0)];
        assert_eq!(bounds_for_points(&pts), rect(0.0, 5.0, 20.0, 25.0));
    }

    // ----- segmentIntersectsRect -----

    #[test]
    fn segment_intersects_rect_horizontal_through() {
        // y=20 through rect {10,10,20,20}
        assert!(segment_intersects_rect(
            &pt(0.0, 20.0), &pt(40.0, 20.0),
            &rect(10.0, 10.0, 20.0, 20.0), 0.0
        ));
    }

    #[test]
    fn segment_intersects_rect_horizontal_outside() {
        // y=5 above rect {10,10,20,20}
        assert!(!segment_intersects_rect(
            &pt(0.0, 5.0), &pt(40.0, 5.0),
            &rect(10.0, 10.0, 20.0, 20.0), 0.0
        ));
    }

    #[test]
    fn segment_intersects_rect_vertical_through() {
        // x=20 through rect {10,10,20,20}
        assert!(segment_intersects_rect(
            &pt(20.0, 0.0), &pt(20.0, 40.0),
            &rect(10.0, 10.0, 20.0, 20.0), 0.0
        ));
    }

    #[test]
    fn segment_intersects_rect_diagonal_always_false() {
        // Diagonal: neither h nor v → false even if it crosses the rect
        assert!(!segment_intersects_rect(
            &pt(0.0, 0.0), &pt(40.0, 40.0),
            &rect(10.0, 10.0, 20.0, 20.0), 0.0
        ));
    }

    #[test]
    fn segment_intersects_rect_on_boundary_is_false() {
        // y exactly equals top boundary (10) → strict > fails → false
        assert!(!segment_intersects_rect(
            &pt(0.0, 10.0), &pt(40.0, 10.0),
            &rect(10.0, 10.0, 20.0, 20.0), 0.0
        ));
    }

    // ----- bendCount -----

    #[test]
    fn bend_count_l_shape() {
        // [0,0]→[10,0]→[10,10]: 1 bend at the corner
        assert_eq!(bend_count(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]), 1);
    }

    #[test]
    fn bend_count_straight_line() {
        assert_eq!(bend_count(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(20.0, 0.0)]), 0);
    }

    #[test]
    fn bend_count_z_shape() {
        // [0,0]→[10,0]→[10,10]→[20,10]: 2 bends
        assert_eq!(
            bend_count(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(20.0, 10.0)]),
            2
        );
    }

    #[test]
    fn bend_count_single_point_or_two_points_is_zero() {
        assert_eq!(bend_count(&[pt(0.0, 0.0)]), 0);
        assert_eq!(bend_count(&[pt(0.0, 0.0), pt(10.0, 0.0)]), 0);
    }

    // ----- shallowJogCount -----

    #[test]
    fn shallow_jog_count_horizontal_jog() {
        // before.y == start.y, end.y == after.y, start.x == end.x, middleLength=10 < 36
        let pts = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(20.0, 10.0)];
        assert_eq!(shallow_jog_count(&pts), 1);
    }

    #[test]
    fn shallow_jog_count_long_middle_not_counted() {
        // middleLength = 40 >= 36: not a shallow jog
        let pts = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 40.0), pt(20.0, 40.0)];
        assert_eq!(shallow_jog_count(&pts), 0);
    }

    #[test]
    fn shallow_jog_count_vertical_jog() {
        // before.x == start.x, end.x == after.x, start.y == end.y, middleLength=10 < 36
        let pts = vec![pt(0.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0), pt(10.0, 20.0)];
        assert_eq!(shallow_jog_count(&pts), 1);
    }

    // ----- routeLength -----

    #[test]
    fn route_length_single_segment() {
        assert_eq!(route_length(&[pt(0.0, 0.0), pt(3.0, 4.0)]), 5.0);
    }

    #[test]
    fn route_length_two_segments() {
        // [0,0]→[10,0]→[10,10]: 10 + 10 = 20
        assert_eq!(route_length(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]), 20.0);
    }

    #[test]
    fn route_length_single_point_is_zero() {
        assert_eq!(route_length(&[pt(0.0, 0.0)]), 0.0);
    }

    // ----- pointAtDistance -----

    #[test]
    fn point_at_distance_midway_first_segment() {
        let seg = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        assert_eq!(point_at_distance(&seg, 5.0), Some(pt(5.0, 0.0)));
    }

    #[test]
    fn point_at_distance_at_segment_boundary() {
        let seg = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        // distance = 10 exactly: traveled+segmentLength (10) >= 10, t=0 → start of second segment
        assert_eq!(point_at_distance(&seg, 10.0), Some(pt(10.0, 0.0)));
    }

    #[test]
    fn point_at_distance_midway_second_segment() {
        let seg = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        assert_eq!(point_at_distance(&seg, 15.0), Some(pt(10.0, 5.0)));
    }

    #[test]
    fn point_at_distance_beyond_total_returns_last() {
        let seg = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)];
        assert_eq!(point_at_distance(&seg, 100.0), Some(pt(10.0, 10.0)));
    }

    #[test]
    fn point_at_distance_empty_is_none() {
        assert_eq!(point_at_distance(&[], 5.0), None);
    }

    #[test]
    fn point_at_distance_zero_distance_returns_first_point() {
        let seg = vec![pt(5.0, 5.0), pt(10.0, 5.0)];
        assert_eq!(point_at_distance(&seg, 0.0), Some(pt(5.0, 5.0)));
    }

    // ----- uniqueRounded -----

    #[test]
    fn unique_rounded_deduplicates_after_rounding() {
        // [1.2, 1.7, 2.3, 1.5, 3.0] → rounds to [1, 2, 2, 2, 3] → deduped [1, 2, 3]
        assert_eq!(unique_rounded(&[1.2, 1.7, 2.3, 1.5, 3.0]), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn unique_rounded_negative_half_deduplicates_with_positive_half() {
        // Math.round(-0.5) = 0 (JS -0), Math.round(0.5) = 1; Set treats -0 same as 0
        // [0.4, 0.6, -0.5, -0.4] → [0, 1, 0, 0] → deduped [0, 1]
        let result = unique_rounded(&[0.4, 0.6, -0.5, -0.4]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 0.0);
        assert_eq!(result[1], 1.0);
    }
}
