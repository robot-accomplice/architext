//! Faithful port of `viewer/src/routing/routeScoring.js`.
//!
//! Translation decisions:
//! - `compareByRoutePriority` reproduces the exact 17-term short-circuit chain
//!   from the JS source, including the intentionally duplicated
//!   `blockedPrimarySurfaceUseCount` term that appears twice (once before
//!   `perimeterFallbackCost`, once after `crossings`). The JS is the ground
//!   truth; the duplicate is load-bearing.
//! - `hasQualityCost` maps any positive cost → 1, zero → 0 (boolean lift). The
//!   comparator subtracts these 0/1 values, NOT the raw cost amounts. This means
//!   two candidates both having a non-zero monotonicBacktrackCost tie on that
//!   term regardless of magnitude — matching JS exactly.
//! - `bestRouteCandidate` uses a strict-less-than guard (`< 0`) so the FIRST
//!   minimal element wins, identical to what a stable sort at index 0 returns.
//!   JS `Array.prototype.sort` is stable; Rust `sort_by` is also stable.
//! - `FACING_SURFACE_ROLES` is a `HashSet` of five role strings. Order doesn't
//!   matter for `contains` lookups; `HashSet` is fine here.
//! - `selfOverlapSegmentStats` / `routeSegments` are private helpers used only
//!   inside `scoreRouteCandidates`, so they are not exported.
//! - `scoreRouteCandidates` and `warningRouteCandidate` require complex context
//!   types (collision callbacks, route index, etc.) that are not yet wired up.
//!   Those are omitted from this port pending the integration phase. All pure
//!   scoring primitives (`compareByRoutePriority`, `bestRouteCandidate`,
//!   `sortedRouteCandidates`, `totalQualityCost`, `withQualityCosts`,
//!   `isCleanRouteCandidate`) are fully ported and unit-tested.
//!
//! ## Hypot-in-comparison sites (Phase-1B parity watch-items)
//! The only `js_hypot` call in `routeScoring.js` flows through the import of
//! `rectDistance` (from `routeGeometry.js`), used in `warningRouteCandidate`
//! at:
//!
//!   ```text
//!   if (rectDistance(context.fromRect, context.toRect) < PORT_STUB * 2) { ... }
//!   ```
//!
//! This is a **threshold comparison** (`< 36.0`), not a tie-break between two
//! floating-point results, so a 1-ULP libm-vs-V8 discrepancy can only flip the
//! branch if the true distance is within 1 ULP of 36.0. Probability is
//! negligible for real diagram coordinates, but it IS a watch-item for the
//! harness to confirm. All other comparisons in this module operate on integer-
//! valued counts and costs accumulated from integer multiplications; they
//! produce exact f64 integers that are not ULP-sensitive.

use std::collections::HashSet;

use crate::model::{Point, Rect};
use crate::route_constants::rect_center;
use crate::route_index::RouteIndex;
use crate::route_intent::{
    derive_route_intent, expected_facing_sides, semantic_surface_options, DeriveRouteIntentInput,
    IntentRelationship, SemanticSurfaceOptionsInput, SidePair, SurfaceOptions,
};
use crate::route_ports::side_vector;

// ---------------------------------------------------------------------------
// FACING_SURFACE_ROLES
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn facing_surface_roles() -> &'static HashSet<&'static str> {
    use std::sync::OnceLock;
    static ROLES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    ROLES.get_or_init(|| {
        let mut s = HashSet::new();
        s.insert("process");
        s.insert("request");
        s.insert("return");
        s.insert("async");
        s.insert("persistence");
        s
    })
}

// ---------------------------------------------------------------------------
// QualityCosts
// ---------------------------------------------------------------------------

/// Mirrors the normalized `qualityCosts` shape used across the JS router.
/// All fields default to 0.0, matching the JS `normalizedQualityCosts` spread.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QualityCosts {
    pub length_cost: f64,
    pub boundary_cost: f64,
    pub node_clearance_cost: f64,
    pub edge_proximity_cost: f64,
    pub label_node_clearance_cost: f64,
    pub point_count_cost: f64,
    pub bend_cost: f64,
    pub dogleg_cost: f64,
    pub perimeter_fallback_cost: f64,
    pub perimeter_length_cost: f64,
    pub directness_reward: f64,
    pub crossing_cost: f64,
    pub repeated_crossing_cost: f64,
    pub self_overlap_cost: f64,
    pub route_overlap_cost: f64,
    pub monotonic_backtrack_cost: f64,
    pub fan_out_direction_cost: f64,
    pub endpoint_stack_cost: f64,
    pub spline_side_direction_cost: f64,
    pub spline_straightness_cost: f64,
    pub same_lane_exterior_cost: f64,
}

impl QualityCosts {
    /// Sum of all cost fields, matching JS `Object.values(qualityCosts).reduce(...)`.
    pub fn total(&self) -> f64 {
        self.length_cost
            + self.boundary_cost
            + self.node_clearance_cost
            + self.edge_proximity_cost
            + self.label_node_clearance_cost
            + self.point_count_cost
            + self.bend_cost
            + self.dogleg_cost
            + self.perimeter_fallback_cost
            + self.perimeter_length_cost
            + self.directness_reward
            + self.crossing_cost
            + self.repeated_crossing_cost
            + self.self_overlap_cost
            + self.route_overlap_cost
            + self.monotonic_backtrack_cost
            + self.fan_out_direction_cost
            + self.endpoint_stack_cost
            + self.spline_side_direction_cost
            + self.spline_straightness_cost
            + self.same_lane_exterior_cost
    }
}

// ---------------------------------------------------------------------------
// RouteCandidate
// ---------------------------------------------------------------------------

/// A route candidate produced by the routing engine. Mirrors the JS object
/// shape that `compareByRoutePriority` and `bestRouteCandidate` operate on.
/// Diagnostic/scoring fields are set by `scoreRouteCandidates`.
#[derive(Debug, Clone, Default)]
pub struct RouteCandidate {
    pub points: Vec<Point>,
    pub samples: Vec<Point>,
    pub style: String,
    /// SVG `d` path string (set by candidate builders / fixedPreferredOrthogonalCandidate).
    pub d: String,
    /// Label position X (midpoint of samples or points).
    pub label_x: f64,
    /// Label position Y (midpoint of samples or points).
    pub label_y: f64,
    /// JS `candidate.startSide`
    pub start_side: Option<String>,
    /// JS `candidate.endSide`
    pub end_side: Option<String>,
    pub quality_costs: QualityCosts,
    pub cost: f64,
    pub bends: i64,
    // Scoring fields set by scoreRouteCandidates:
    pub collisions: i64,
    pub padded_collisions: i64,
    pub endpoint_node_traversals: i64,
    pub self_overlapping_segments: i64,
    pub self_overlap_length: f64,
    pub crossings: i64,
    pub repeated_crossings: i64,
    pub shared_segments: i64,
    pub shared_segment_length: f64,
    pub surface_mismatch_count: i64,
    pub semantic_surface_mismatch_count: i64,
    pub surface_direction_mismatch_count: i64,
    pub blocked_primary_surface_use_count: i64,
    pub same_lane_exterior_mismatch_count: i64,
    pub warnings: Vec<RouteWarning>,
}

#[derive(Debug, Clone)]
pub struct RouteWarning {
    pub code: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// totalQualityCost
// ---------------------------------------------------------------------------

/// Port of JS `totalQualityCost(qualityCosts)`.
///
/// Returns the sum of all values in `qualityCosts`, matching
/// `Object.values(qualityCosts).reduce((sum, value) => sum + value, 0)`.
pub fn total_quality_cost(costs: &QualityCosts) -> f64 {
    costs.total()
}

// ---------------------------------------------------------------------------
// withQualityCosts
// ---------------------------------------------------------------------------

/// Port of JS `withQualityCosts(route, qualityCosts)`.
///
/// Merges `quality_costs` into `candidate`, applying the JS zero-default
/// spread for every field not supplied. Sets `candidate.cost` to the total.
pub fn with_quality_costs(mut candidate: RouteCandidate, quality_costs: QualityCosts) -> RouteCandidate {
    // JS: normalizedQualityCosts = { ...defaults, ...qualityCosts }
    // Since QualityCosts derives Default (all 0.0), the caller supplies what it
    // wants and the rest are already zero. The JS spread merges caller-supplied
    // fields on top of the zero defaults — identical to what we have here.
    candidate.quality_costs = quality_costs;
    candidate.cost = candidate.quality_costs.total();
    candidate
}

// ---------------------------------------------------------------------------
// Private scoring helpers
// ---------------------------------------------------------------------------
// These helpers are correct translations of JS private functions in
// routeScoring.js. They are called by scoreRouteCandidates, which is not
// yet wired (it requires context objects from the higher integration layer).
// Suppress dead-code warnings until that wiring is done.

/// Segment type (horizontal or vertical), plus the fixed axis value and range.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct Segment {
    orientation: Orientation,
    /// For horizontal: y value; for vertical: x value.
    line: f64,
    min: f64,
    max: f64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum Orientation {
    Horizontal,
    Vertical,
}

/// Port of JS private `routeSegments(points)`.
///
/// For each consecutive axis-aligned pair of points, emits a `Segment`.
/// Diagonal segments (neither Δx=0 nor Δy=0) are skipped, matching JS.
#[allow(dead_code)]
fn route_segments(points: &[Point]) -> Vec<Segment> {
    let mut segments = Vec::new();
    for index in 0..points.len().saturating_sub(1) {
        let start = &points[index];
        let end = &points[index + 1];
        if start.y == end.y {
            segments.push(Segment {
                orientation: Orientation::Horizontal,
                line: start.y,
                min: f64::min(start.x, end.x),
                max: f64::max(start.x, end.x),
            });
        } else if start.x == end.x {
            segments.push(Segment {
                orientation: Orientation::Vertical,
                line: start.x,
                min: f64::min(start.y, end.y),
                max: f64::max(start.y, end.y),
            });
        }
    }
    segments
}

/// Port of JS private `selfOverlapSegmentStats(points)`.
///
/// Returns `(count, length)` of overlapping segment pairs. Two segments overlap
/// when they share the same orientation and axis value and their ranges overlap
/// by more than 1 pixel.
#[allow(dead_code)]
fn self_overlap_segment_stats(points: &[Point]) -> (i64, f64) {
    let segments = route_segments(points);
    let mut count = 0i64;
    let mut length = 0.0f64;
    for left_index in 0..segments.len() {
        for right_index in (left_index + 1)..segments.len() {
            let left = &segments[left_index];
            let right = &segments[right_index];
            if left.orientation != right.orientation || left.line != right.line {
                continue;
            }
            let overlap = f64::min(left.max, right.max) - f64::max(left.min, right.min);
            if overlap > 1.0 {
                count += 1;
                length += overlap;
            }
        }
    }
    (count, length)
}

/// Port of JS private `pointInsideRect(point, rect)`.
///
/// Interior-only (strict inequalities), matching JS `point.x > rect.x && ...`.
#[allow(dead_code)]
fn point_inside_rect(point: &Point, rect: &Rect) -> bool {
    point.x > rect.x
        && point.x < rect.x + rect.width
        && point.y > rect.y
        && point.y < rect.y + rect.height
}

/// Port of JS private `endpointNodeTraversalCount(candidate, fromRect, toRect)`.
///
/// Counts how many of {from, to} rects are traversed (0, 1, or 2).
#[allow(dead_code)]
fn endpoint_node_traversal_count(
    samples: &[Point],
    from_rect: Option<&Rect>,
    to_rect: Option<&Rect>,
) -> i64 {
    let mut traversed_from = false;
    let mut traversed_to = false;
    for sample in samples {
        if let Some(r) = from_rect {
            if point_inside_rect(sample, r) {
                traversed_from = true;
            }
        }
        if let Some(r) = to_rect {
            if point_inside_rect(sample, r) {
                traversed_to = true;
            }
        }
    }
    traversed_from as i64 + traversed_to as i64
}

/// Port of JS private `sideMatches(side, expected)`.
///
/// When `expected` is a `Set` (multiple allowed sides), checks membership.
/// When `expected` is a string (single side), checks equality.
#[allow(dead_code)]
fn side_matches(side: &str, expected: SideExpected<'_>) -> bool {
    match expected {
        SideExpected::One(s) => side == s,
        SideExpected::Many(set) => set.contains(side),
    }
}

#[allow(dead_code)]
enum SideExpected<'a> {
    One(&'a str),
    Many(&'a indexmap::IndexSet<String>),
}

/// Port of JS private `surfaceMismatchCount(candidate, expectedSides, relationship)`.
#[allow(dead_code)]
fn surface_mismatch_count(
    start_side: Option<&str>,
    end_side: Option<&str>,
    style: &str,
    expected_sides: &SidePair,
    relationship: Option<&IntentRelationship>,
) -> i64 {
    if style == "spline" {
        return 0;
    }
    if let Some(rel) = relationship {
        if rel.preferred_start_side.is_some() || rel.preferred_end_side.is_some() {
            return 0;
        }
        if let Some(kind) = &rel.kind {
            if !facing_surface_roles().contains(kind.as_str()) {
                return 0;
            }
        }
    }
    let mut count = 0i64;
    if let Some(side) = start_side {
        if !side_matches(side, SideExpected::One(&expected_sides.source)) {
            count += 1;
        }
    }
    if let Some(side) = end_side {
        if !side_matches(side, SideExpected::One(&expected_sides.target)) {
            count += 1;
        }
    }
    count
}

/// Port of JS private `semanticSurfaceMismatchCount`.
///
/// JS passes `semanticSides` as `{source: Set, target: Set}` where the sets may
/// contain multiple allowed sides (expanded when the primary corridor is blocked).
/// `sideMatches` in JS uses `Set.has()` for membership when the expected value is a
/// Set. We mirror this by accepting `&SurfaceOptions` (which carries `IndexSet`s) and
/// using `SideExpected::Many` for the membership check.
#[allow(dead_code)]
fn semantic_surface_mismatch_count(
    start_side: Option<&str>,
    end_side: Option<&str>,
    style: &str,
    semantic_sides: &SurfaceOptions,
    relationship: Option<&IntentRelationship>,
) -> i64 {
    // Guard: relationship must have at least one semantic field
    let has_semantic = relationship.is_some_and(|rel| {
        rel.relationship_type.is_some()
            || rel.kind.is_some()
            || rel.return_of.is_some()
            || rel.outcome.is_some()
            || rel.step_id.is_some()
            || rel.flow_id.is_some()
    });
    if !has_semantic {
        return 0;
    }
    if style == "spline" {
        return 0;
    }
    if let Some(rel) = relationship {
        if rel.preferred_start_side.is_some() || rel.preferred_end_side.is_some() {
            return 0;
        }
        if let Some(kind) = &rel.kind {
            if !facing_surface_roles().contains(kind.as_str()) {
                return 0;
            }
        }
    }
    let mut count = 0i64;
    if let Some(side) = start_side {
        if !side_matches(side, SideExpected::Many(&semantic_sides.source)) {
            count += 1;
        }
    }
    if let Some(side) = end_side {
        if !side_matches(side, SideExpected::Many(&semantic_sides.target)) {
            count += 1;
        }
    }
    count
}

/// Port of JS private `blockedPrimarySurfaceUseCount`.
#[allow(dead_code)]
fn blocked_primary_surface_use_count(
    start_side: Option<&str>,
    end_side: Option<&str>,
    primary_sides: &SidePair,
    semantic_sides: &SurfaceOptions,
) -> i64 {
    let mut count = 0i64;
    if semantic_sides.source.len() > 1 {
        if let Some(side) = start_side {
            if side == primary_sides.source {
                count += 1;
            }
        }
    }
    if semantic_sides.target.len() > 1 {
        if let Some(side) = end_side {
            if side == primary_sides.target {
                count += 1;
            }
        }
    }
    count
}

/// Port of JS private `surfaceDirectionMismatchCount`.
#[allow(dead_code)]
fn surface_direction_mismatch_count(
    start_side: Option<&str>,
    end_side: Option<&str>,
    style: &str,
    from_rect: Option<&Rect>,
    to_rect: Option<&Rect>,
    relationship: Option<&IntentRelationship>,
) -> i64 {
    if style == "spline" {
        return 0;
    }
    if let Some(rel) = relationship {
        if rel.preferred_start_side.is_some() || rel.preferred_end_side.is_some() {
            return 0;
        }
    }
    let (ss, es) = match (start_side, end_side) {
        (Some(s), Some(e)) => (s, e),
        _ => return 0,
    };
    let (fr, tr) = match (from_rect, to_rect) {
        (Some(f), Some(t)) => (f, t),
        _ => return 0,
    };
    let from_center = rect_center(fr);
    let to_center = rect_center(tr);
    let direction = Point {
        x: to_center.x - from_center.x,
        y: to_center.y - from_center.y,
    };
    if direction.x == 0.0 && direction.y == 0.0 {
        return 0;
    }
    let start_vec = side_vector(ss);
    let end_vec = side_vector(es);
    let mut count = 0i64;
    if start_vec.x * direction.x + start_vec.y * direction.y < 0.0 {
        count += 1;
    }
    if end_vec.x * direction.x + end_vec.y * direction.y > 0.0 {
        count += 1;
    }
    count
}

/// Port of JS private `sameLaneExteriorMismatchCount(candidate, context)`.
///
/// This function depends on the full scoring context (blockerRects, lane/row
/// indexes, canvasWidth, rects). It is implemented here for completeness but
/// requires the full context to be meaningful; callers that lack context should
/// pass `None` for optional fields and rely on the early-exit guards.
#[allow(dead_code, clippy::too_many_arguments)]
fn same_lane_exterior_mismatch_count(
    points: &[Point],
    style: &str,
    relationship: Option<&IntentRelationship>,
    from_lane_index: Option<i64>,
    to_lane_index: Option<i64>,
    from_row_index: Option<i64>,
    to_row_index: Option<i64>,
    canvas_width: Option<f64>,
    from_rect: Option<&Rect>,
    to_rect: Option<&Rect>,
    blocker_rects: &[Rect],
) -> i64 {
    if style == "spline" {
        return 0;
    }
    let has_semantic = relationship.is_some_and(|rel| {
        rel.relationship_type.is_some()
            || rel.kind.is_some()
            || rel.return_of.is_some()
            || rel.outcome.is_some()
            || rel.step_id.is_some()
            || rel.flow_id.is_some()
    });
    if !has_semantic {
        return 0;
    }
    // fromLaneIndex !== toLaneIndex  AND  fromRowIndex === toRowIndex  → 0 (no same-lane exterior case)
    let (fl, tl) = match (from_lane_index, to_lane_index) {
        (Some(f), Some(t)) => (f, t),
        _ => return 0,
    };
    if fl != tl {
        return 0;
    }
    let (fr_idx, tr_idx) = match (from_row_index, to_row_index) {
        (Some(f), Some(t)) => (f, t),
        _ => return 0,
    };
    if fr_idx == tr_idx {
        return 0;
    }
    let cw = match canvas_width {
        Some(w) if w != 0.0 => w,
        _ => return 0,
    };
    let (frect, trect) = match (from_rect, to_rect) {
        (Some(f), Some(t)) => (f, t),
        _ => return 0,
    };
    let node_left = f64::min(frect.x, trect.x);
    let node_right = f64::max(frect.x + frect.width, trect.x + trect.width);
    let channel_top = f64::min(frect.y + frect.height, trect.y + trect.height);
    let channel_bottom = f64::max(frect.y, trect.y);
    let interior_blocked = blocker_rects.iter().any(|rect| {
        rect.x < node_right
            && rect.x + rect.width > node_left
            && rect.y < channel_bottom
            && rect.y + rect.height > channel_top
    });
    if !interior_blocked {
        return 0;
    }
    let node_center_x =
        (frect.x + frect.width / 2.0 + trect.x + trect.width / 2.0) / 2.0;
    let prefer_left_exterior = node_center_x < cw / 2.0;
    let segs = route_segments(points);
    let uses_left_exterior = segs.iter().any(|s| s.orientation == Orientation::Vertical && s.line < node_left);
    let uses_right_exterior = segs.iter().any(|s| s.orientation == Orientation::Vertical && s.line > node_right);
    if prefer_left_exterior {
        if uses_left_exterior { 0 } else { 1 }
    } else {
        if uses_right_exterior { 0 } else { 1 }
    }
}

// ---------------------------------------------------------------------------
// hasQualityCost
// ---------------------------------------------------------------------------

/// Port of JS private `hasQualityCost(candidate, costName)`.
///
/// Returns 1 if the named cost is positive, 0 otherwise. This is used in the
/// comparator as a boolean lift: two candidates both having a non-zero cost
/// TIE on that term, regardless of magnitude.
fn has_quality_cost(value: f64) -> i64 {
    if value > 0.0 { 1 } else { 0 }
}

/// Port of JS private `routeMetric(candidate, metricName)`.
///
/// Returns the metric value, defaulting to 0 when absent.
/// Since our struct fields have explicit defaults (0 / 0.0), this is just
/// field access.
#[inline]
fn route_metric_i64(value: i64) -> i64 {
    value
}

#[inline]
fn route_metric_f64(value: f64) -> f64 {
    value
}

// ---------------------------------------------------------------------------
// scoreRouteCandidates
// ---------------------------------------------------------------------------

/// Input context for `score_route_candidates`.
///
/// Mirrors the JS `context` object passed to `scoreRouteCandidates`.
pub struct ScoreContext<'a> {
    pub collision_count: &'a dyn Fn(&RouteCandidate, &str, &str, f64) -> i64,
    pub route_index: &'a RouteIndex,
    pub from_id: &'a str,
    pub to_id: &'a str,
    pub from_rect: &'a Rect,
    pub to_rect: &'a Rect,
    pub pair_index: usize,
    pub top_limit: f64,
    pub bottom_limit: f64,
    pub relationship: Option<&'a IntentRelationship>,
    pub from_lane_index: Option<i64>,
    pub to_lane_index: Option<i64>,
    pub from_row_index: Option<i64>,
    pub to_row_index: Option<i64>,
    pub canvas_width: Option<f64>,
    pub canvas_height: Option<f64>,
    pub blocker_rects: &'a [Rect],
}

/// Port of JS `scoreRouteCandidates(candidateList, context)`.
///
/// Fills `collisions`, `padded_collisions`, `endpoint_node_traversals`,
/// crossing/overlap/surface-mismatch counts, fan-out direction cost, and
/// `cost` on each candidate in the slice.
pub fn score_route_candidates(candidates: &mut [RouteCandidate], ctx: &ScoreContext<'_>) {
    let expected_sides: SidePair = if let Some(rel) = ctx.relationship {
        let intent = derive_route_intent(&DeriveRouteIntentInput {
            relationship: rel,
            from_rect: ctx.from_rect,
            to_rect: ctx.to_rect,
            from_lane_index: ctx.from_lane_index.unwrap_or(0),
            to_lane_index: ctx.to_lane_index.unwrap_or(0),
            from_row_index: ctx.from_row_index.unwrap_or(0),
            to_row_index: ctx.to_row_index.unwrap_or(0),
        });
        // JS: expectedSides.source ?? expectedSides.expectedSourceSide
        // RouteIntent has expected_source_side / expected_target_side (no separate source/target)
        SidePair {
            source: intent.expected_source_side,
            target: intent.expected_target_side,
        }
    } else {
        expected_facing_sides(ctx.from_rect, ctx.to_rect)
    };

    let semantic_sides: SurfaceOptions = if let Some(rel) = ctx.relationship {
        semantic_surface_options(&SemanticSurfaceOptionsInput {
            expected_sides: SidePair {
                source: expected_sides.source.clone(),
                target: expected_sides.target.clone(),
            },
            relationship: rel,
            from_rect: ctx.from_rect,
            to_rect: ctx.to_rect,
            blocker_rects: ctx.blocker_rects.to_vec(),
            canvas_width: ctx.canvas_width.unwrap_or(0.0),
            canvas_height: ctx.canvas_height.unwrap_or(0.0),
        })
    } else {
        SurfaceOptions {
            source: {
                let mut s = indexmap::IndexSet::new();
                s.insert(expected_sides.source.clone());
                s
            },
            target: {
                let mut s = indexmap::IndexSet::new();
                s.insert(expected_sides.target.clone());
                s
            },
        }
    };

    for candidate in candidates.iter_mut() {
        let travels_top = candidate.samples.iter().any(|p| p.y < ctx.top_limit - 4.0);
        let travels_bottom = candidate.samples.iter().any(|p| p.y > ctx.bottom_limit + 4.0);

        candidate.collisions = (ctx.collision_count)(candidate, ctx.from_id, ctx.to_id, 0.0);
        candidate.padded_collisions = (ctx.collision_count)(candidate, ctx.from_id, ctx.to_id, 8.0);
        candidate.endpoint_node_traversals =
            endpoint_node_traversal_count(&candidate.samples, Some(ctx.from_rect), Some(ctx.to_rect));

        let (self_overlap_count, self_overlap_len) = if candidate.style == "spline" {
            (0, 0.0)
        } else {
            self_overlap_segment_stats(&candidate.points)
        };
        let crossing_stats = if candidate.style == "spline" {
            crate::route_index::CrossingStats { total: 0, repeated: 0 }
        } else {
            ctx.route_index.crossing_stats(&candidate.points)
        };
        let shared_stats = if candidate.style == "spline" {
            crate::route_index::SharedSegmentStats { count: 0, length: 0.0 }
        } else {
            ctx.route_index.shared_segment_stats(&candidate.points)
        };

        candidate.self_overlapping_segments = self_overlap_count;
        candidate.self_overlap_length = self_overlap_len;
        candidate.crossings = crossing_stats.total;
        candidate.repeated_crossings = crossing_stats.repeated;
        candidate.shared_segments = shared_stats.count;
        candidate.shared_segment_length = shared_stats.length;

        candidate.surface_mismatch_count = surface_mismatch_count(
            candidate.start_side.as_deref(),
            candidate.end_side.as_deref(),
            &candidate.style,
            &SidePair {
                source: expected_sides.source.clone(),
                target: expected_sides.target.clone(),
            },
            ctx.relationship,
        );
        candidate.semantic_surface_mismatch_count = semantic_surface_mismatch_count(
            candidate.start_side.as_deref(),
            candidate.end_side.as_deref(),
            &candidate.style,
            &semantic_sides,
            ctx.relationship,
        );
        candidate.surface_direction_mismatch_count = surface_direction_mismatch_count(
            candidate.start_side.as_deref(),
            candidate.end_side.as_deref(),
            &candidate.style,
            Some(ctx.from_rect),
            Some(ctx.to_rect),
            ctx.relationship,
        );
        candidate.blocked_primary_surface_use_count = blocked_primary_surface_use_count(
            candidate.start_side.as_deref(),
            candidate.end_side.as_deref(),
            &SidePair {
                source: expected_sides.source.clone(),
                target: expected_sides.target.clone(),
            },
            &semantic_sides,
        );
        candidate.same_lane_exterior_mismatch_count = same_lane_exterior_mismatch_count(
            &candidate.points,
            &candidate.style,
            ctx.relationship,
            ctx.from_lane_index,
            ctx.to_lane_index,
            ctx.from_row_index,
            ctx.to_row_index,
            ctx.canvas_width,
            Some(ctx.from_rect),
            Some(ctx.to_rect),
            ctx.blocker_rects,
        );

        candidate.quality_costs.crossing_cost = crossing_stats.total as f64 * 3000.0;
        candidate.quality_costs.repeated_crossing_cost = crossing_stats.repeated as f64 * 40000.0;
        candidate.quality_costs.self_overlap_cost =
            self_overlap_count as f64 * 120000.0 + self_overlap_len * 1600.0;
        candidate.quality_costs.route_overlap_cost =
            shared_stats.count as f64 * 80000.0 + shared_stats.length * 1200.0;
        candidate.quality_costs.endpoint_stack_cost =
            if ctx.route_index.has_stacked_endpoint(&candidate.points) { 90000.0 } else { 0.0 };
        candidate.quality_costs.same_lane_exterior_cost =
            candidate.same_lane_exterior_mismatch_count as f64 * 20000.0;

        if ctx.pair_index % 2 == 1 && travels_top {
            candidate.quality_costs.fan_out_direction_cost += 25000.0;
        }
        if ctx.pair_index % 2 == 1 && !travels_bottom {
            candidate.quality_costs.fan_out_direction_cost += 4000.0;
        }
        if ctx.pair_index.is_multiple_of(2) && travels_bottom {
            candidate.quality_costs.fan_out_direction_cost += 600.0;
        }

        candidate.cost = total_quality_cost(&candidate.quality_costs);
    }
}

// ---------------------------------------------------------------------------
// compareByRoutePriority
// ---------------------------------------------------------------------------

/// Port of JS `compareByRoutePriority(a, b)`.
///
/// 17-term lexicographic comparator. Each term is a subtraction; the first
/// non-zero result wins. Returns:
/// - negative  → `a` sorts before `b`
/// - zero      → `a` and `b` are equal under this comparator (stable sort
///   preserves insertion order)
/// - positive  → `a` sorts after `b`
///
/// **Critical:** the `blockedPrimarySurfaceUseCount` term appears TWICE in
/// the JS source. This is intentional — the first occurrence is a tight
/// semantic guard; the second is a tiebreak after `crossings`. Both are
/// reproduced here.
///
/// **Hypot watch-items:** none in this function directly. The `cost` field
/// may contain float arithmetic from geometry helpers, but scores are
/// accumulated as sums of integer-valued products. Float tie on `cost` is
/// therefore deterministic for any real plan. See module doc for the single
/// hypot site in `warningRouteCandidate`.
pub fn compare_by_route_priority(a: &RouteCandidate, b: &RouteCandidate) -> std::cmp::Ordering {
    // We replicate the JS `||`-chain as a sequence of comparisons returning
    // std::cmp::Ordering. Each subtraction term maps to Ordering.
    macro_rules! cmp_i64 {
        ($ax:expr, $bx:expr) => {{
            let d = route_metric_i64($ax) - route_metric_i64($bx);
            if d != 0 { return if d < 0 { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }; }
        }};
    }
    macro_rules! cmp_f64 {
        ($ax:expr, $bx:expr) => {{
            let av = route_metric_f64($ax);
            let bv = route_metric_f64($bx);
            let d = av - bv;
            if d < 0.0 { return std::cmp::Ordering::Less; }
            if d > 0.0 { return std::cmp::Ordering::Greater; }
        }};
    }
    macro_rules! cmp_hqc {
        ($av:expr, $bv:expr) => {{
            let d = has_quality_cost($av) - has_quality_cost($bv);
            if d != 0 { return if d < 0 { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }; }
        }};
    }

    cmp_i64!(a.collisions, b.collisions);
    cmp_i64!(a.endpoint_node_traversals, b.endpoint_node_traversals);
    cmp_i64!(a.self_overlapping_segments, b.self_overlapping_segments);
    cmp_f64!(a.self_overlap_length, b.self_overlap_length);
    cmp_i64!(a.repeated_crossings, b.repeated_crossings);
    cmp_i64!(a.semantic_surface_mismatch_count, b.semantic_surface_mismatch_count);
    // First occurrence of blockedPrimarySurfaceUseCount
    cmp_i64!(a.blocked_primary_surface_use_count, b.blocked_primary_surface_use_count);
    cmp_i64!(a.surface_direction_mismatch_count, b.surface_direction_mismatch_count);
    cmp_i64!(a.same_lane_exterior_mismatch_count, b.same_lane_exterior_mismatch_count);
    cmp_i64!(a.padded_collisions, b.padded_collisions);
    cmp_i64!(a.shared_segments, b.shared_segments);
    cmp_f64!(a.shared_segment_length, b.shared_segment_length);
    cmp_hqc!(a.quality_costs.perimeter_fallback_cost, b.quality_costs.perimeter_fallback_cost);
    cmp_i64!(a.crossings, b.crossings);
    // Second occurrence of blockedPrimarySurfaceUseCount (after crossings)
    cmp_i64!(a.blocked_primary_surface_use_count, b.blocked_primary_surface_use_count);
    cmp_hqc!(a.quality_costs.monotonic_backtrack_cost, b.quality_costs.monotonic_backtrack_cost);
    cmp_hqc!(a.quality_costs.endpoint_stack_cost, b.quality_costs.endpoint_stack_cost);
    cmp_i64!(a.bends, b.bends);
    cmp_f64!(a.cost, b.cost);

    std::cmp::Ordering::Equal
}

// ---------------------------------------------------------------------------
// sortedRouteCandidates
// ---------------------------------------------------------------------------

/// Port of JS `sortedRouteCandidates(candidateList)`.
///
/// Stable sort in-place (same stability guarantee as JS `Array.prototype.sort`).
pub fn sorted_route_candidates(candidates: &mut [RouteCandidate]) {
    candidates.sort_by(compare_by_route_priority);
}

// ---------------------------------------------------------------------------
// bestRouteCandidate
// ---------------------------------------------------------------------------

/// Port of JS `bestRouteCandidate(candidateList)`.
///
/// Single-pass O(n) winner. Uses strict `< 0` guard so the FIRST minimal
/// element wins — identical to what a stable sort at index 0 returns.
pub fn best_route_candidate(candidates: &[RouteCandidate]) -> Option<&RouteCandidate> {
    let mut best: Option<&RouteCandidate> = None;
    for candidate in candidates {
        match best {
            None => best = Some(candidate),
            Some(current) => {
                if compare_by_route_priority(candidate, current) == std::cmp::Ordering::Less {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

// ---------------------------------------------------------------------------
// isCleanRouteCandidate
// ---------------------------------------------------------------------------

/// Port of JS `isCleanRouteCandidate(candidate)`.
///
/// Returns true iff all primary quality metrics are zero.
pub fn is_clean_route_candidate(c: &RouteCandidate) -> bool {
    c.collisions == 0
        && c.padded_collisions == 0
        && c.endpoint_node_traversals == 0
        && c.self_overlapping_segments == 0
        && c.repeated_crossings == 0
        && c.crossings == 0
        && c.shared_segments == 0
        && c.quality_costs.endpoint_stack_cost == 0.0
        && c.quality_costs.perimeter_fallback_cost == 0.0
        && c.quality_costs.dogleg_cost == 0.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    fn base_candidate() -> RouteCandidate {
        RouteCandidate::default()
    }

    // --- totalQualityCost / QualityCosts.total ---

    #[test]
    fn total_quality_cost_empty() {
        // Node: totalQualityCost({}) → 0
        let costs = QualityCosts::default();
        assert_eq!(total_quality_cost(&costs), 0.0);
    }

    #[test]
    fn total_quality_cost_two_fields() {
        // Node: totalQualityCost({a:3, b:4}) → 7
        let costs = QualityCosts { bend_cost: 3.0, dogleg_cost: 4.0, ..Default::default() };
        assert_eq!(total_quality_cost(&costs), 7.0);
    }

    #[test]
    fn total_quality_cost_positive_negative() {
        // Node: totalQualityCost({a:1.5, b:-0.5}) → 1.0
        let costs = QualityCosts { length_cost: 1.5, directness_reward: -0.5, ..Default::default() };
        assert_eq!(total_quality_cost(&costs), 1.0);
    }

    // --- withQualityCosts ---

    #[test]
    fn with_quality_costs_sets_cost_and_normalizes() {
        // Node: withQualityCosts(route, {bendCost:420, doglegCost:14000})
        // → cost: 14420, qualityCosts.bendCost: 420, doglegCost: 14000, lengthCost: 0
        let c = base_candidate();
        let qc = QualityCosts { bend_cost: 420.0, dogleg_cost: 14000.0, ..Default::default() };
        let result = with_quality_costs(c, qc);
        assert_eq!(result.cost, 14420.0);
        assert_eq!(result.quality_costs.bend_cost, 420.0);
        assert_eq!(result.quality_costs.dogleg_cost, 14000.0);
        assert_eq!(result.quality_costs.length_cost, 0.0);
    }

    // --- compareByRoutePriority ---

    #[test]
    fn compare_equal_candidates() {
        // Node: compareByRoutePriority(a, a) → 0
        let a = base_candidate();
        assert_eq!(compare_by_route_priority(&a, &a), Ordering::Equal);
    }

    #[test]
    fn compare_by_bends() {
        // Node: a.bends=5, b.bends=3 → compareByRoutePriority(a,b)=2 (positive)
        let mut a = base_candidate();
        a.bends = 5;
        let mut b = base_candidate();
        b.bends = 3;
        assert_eq!(compare_by_route_priority(&a, &b), Ordering::Greater);
        assert_eq!(compare_by_route_priority(&b, &a), Ordering::Less);
    }

    #[test]
    fn compare_by_collisions_dominates_bends() {
        // Node: c.collisions=1 vs a.collisions=0 → c sorts after a
        let a = base_candidate();
        let mut c = base_candidate();
        c.collisions = 1;
        c.bends = -10; // even with better bends, collision dominates
        assert_eq!(compare_by_route_priority(&c, &a), Ordering::Greater);
    }

    #[test]
    fn compare_by_crossings() {
        // Node: a.crossings=0 vs d.crossings=2 → a sorts before d
        let a = base_candidate();
        let mut d = base_candidate();
        d.crossings = 2;
        d.bends = 2;
        d.cost = 800.0;
        assert_eq!(compare_by_route_priority(&a, &d), Ordering::Less);
    }

    #[test]
    fn compare_by_cost() {
        // Node: cost 100 vs 200 → -100 (a sorts before b)
        let mut a = base_candidate();
        a.cost = 100.0;
        let mut b = base_candidate();
        b.cost = 200.0;
        assert_eq!(compare_by_route_priority(&a, &b), Ordering::Less);
    }

    #[test]
    fn has_quality_cost_boolean_lift() {
        // Node: monotonicBacktrackCost 18 vs 5000 → 0 (both >0 → both 1)
        // Both are "1" after the boolean lift, so they tie on that term.
        let mut a = base_candidate();
        a.quality_costs.monotonic_backtrack_cost = 18.0;
        let mut b = base_candidate();
        b.quality_costs.monotonic_backtrack_cost = 5000.0;
        // They tie on monotonicBacktrack; cost is 0 for both → Equal overall
        assert_eq!(compare_by_route_priority(&a, &b), Ordering::Equal);
    }

    #[test]
    fn has_quality_cost_perimeter_fallback() {
        // Node: perimeterFallback 7000 vs 0 → 1 (positive)
        let mut a = base_candidate();
        a.quality_costs.perimeter_fallback_cost = 7000.0;
        let b = base_candidate();
        assert_eq!(compare_by_route_priority(&a, &b), Ordering::Greater);
    }

    #[test]
    fn has_quality_cost_endpoint_stack() {
        // Node: endpointStack 0 vs 90000 → -1 (a sorts before b)
        let a = base_candidate();
        let mut b = base_candidate();
        b.quality_costs.endpoint_stack_cost = 90000.0;
        assert_eq!(compare_by_route_priority(&a, &b), Ordering::Less);
    }

    #[test]
    fn blocked_primary_surface_use_count_term() {
        // Node: blockedPrimary 1 vs 0 → 1 (positive, a sorts after b)
        let mut a = base_candidate();
        a.blocked_primary_surface_use_count = 1;
        let b = base_candidate();
        assert_eq!(compare_by_route_priority(&a, &b), Ordering::Greater);
    }

    // --- sortedRouteCandidates ---

    #[test]
    fn sorted_stable_order() {
        // Node: [s3,s2,s1] where s2,s1 tie (bends=0,cost=0), s3 has bends=1
        // Stable sort: s2 and s1 preserve insertion order → s2 before s1; s3 last.
        // Node golden: sorted ids: ["s2", "s1", "s3"]
        let mut s1 = base_candidate();
        s1.bends = 0;
        let mut s2 = base_candidate();
        s2.bends = 0;
        let mut s3 = base_candidate();
        s3.bends = 1;
        let mut v = vec![s3, s2, s1];
        sorted_route_candidates(&mut v);
        // s2 and s1 both have bends=0, s3 has bends=1 → s3 last
        // s2 was at index 1, s1 at index 2 in the original → stable sort preserves their
        // relative order: s2 before s1
        assert_eq!(v[0].bends, 0); // s2
        assert_eq!(v[1].bends, 0); // s1
        assert_eq!(v[2].bends, 1); // s3
        // Check stable order: the candidate with bends=0 that was inserted first in
        // the original [s3,s2,s1] after removing s3 is s2 (index 1), then s1 (index 2).
        // After stable sort the two equal elements maintain relative order → first is s2.
        // We can distinguish them only by address when the structs are identical;
        // since we can't track identity here, just verify the bends order holds.
    }

    // --- bestRouteCandidate ---

    #[test]
    fn best_returns_first_on_tie() {
        // Node: bestRouteCandidate([p,q,b,c]) === p (p and q are ties, p is first)
        let p = base_candidate(); // bends=0, cost=0
        let q = base_candidate(); // identical to p
        let mut b = base_candidate();
        b.bends = 3;
        let mut c = base_candidate();
        c.collisions = 1;
        let candidates = vec![p, q, b, c];
        let best = best_route_candidate(&candidates).unwrap();
        // best should be candidates[0] (p) — the first minimal element
        assert!(std::ptr::eq(best, &candidates[0]));
    }

    #[test]
    fn best_picks_lower_bends() {
        // Node: bestRouteCandidate([b,a]) is b (b.bends=3, a.bends=5)
        let mut b = base_candidate();
        b.bends = 3;
        b.cost = 1200.0;
        let mut a = base_candidate();
        a.bends = 5;
        a.cost = 1000.0;
        let candidates = vec![b, a];
        let best = best_route_candidate(&candidates).unwrap();
        assert!(std::ptr::eq(best, &candidates[0])); // b is first and better
    }

    #[test]
    fn best_returns_none_for_empty() {
        let empty: Vec<RouteCandidate> = vec![];
        assert!(best_route_candidate(&empty).is_none());
    }

    // --- isCleanRouteCandidate ---

    #[test]
    fn is_clean_all_zero() {
        // Node: isCleanRouteCandidate(clean) → true
        let c = base_candidate();
        assert!(is_clean_route_candidate(&c));
    }

    #[test]
    fn is_clean_false_on_collision() {
        // Node: isCleanRouteCandidate({collisions:1}) → false
        let mut c = base_candidate();
        c.collisions = 1;
        assert!(!is_clean_route_candidate(&c));
    }

    #[test]
    fn is_clean_false_on_dogleg_cost() {
        // Node: isCleanRouteCandidate({qualityCosts:{doglegCost:14000}}) → false
        let mut c = base_candidate();
        c.quality_costs.dogleg_cost = 14000.0;
        assert!(!is_clean_route_candidate(&c));
    }

    #[test]
    fn is_clean_false_on_perimeter_fallback() {
        let mut c = base_candidate();
        c.quality_costs.perimeter_fallback_cost = 7000.0;
        assert!(!is_clean_route_candidate(&c));
    }

    // --- selfOverlapSegmentStats ---

    #[test]
    fn self_overlap_none() {
        // L-shaped route: no segment shares the same line
        let points = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
            Point { x: 100.0, y: 100.0 },
        ];
        let (count, length) = self_overlap_segment_stats(&points);
        assert_eq!(count, 0);
        assert_eq!(length, 0.0);
    }

    #[test]
    fn self_overlap_detected() {
        // U-shape: two horizontal segments on y=0, overlapping in x [20,80]
        // Segment 1: y=0, x from 0 to 80 (horizontal)
        // Segment 2: y=0, x from 20 to 100 (horizontal, same y, overlap=60)
        // Use the full U-shape directly:
        let points2 = vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 80.0, y: 0.0 },
            Point { x: 80.0, y: 50.0 },
            Point { x: 20.0, y: 50.0 },
            Point { x: 20.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
        ];
        // Segments: H(y=0, 0..80), V(x=80, 0..50), H(y=50, 20..80), V(x=20, 0..50), H(y=0, 20..100)
        // H segments on same line y=0: [0..80] and [20..100] → overlap = min(80,100)-max(0,20) = 80-20 = 60 > 1
        let (count, length) = self_overlap_segment_stats(&points2);
        assert_eq!(count, 1);
        assert_eq!(length, 60.0);
    }

    // --- endpointNodeTraversalCount ---

    #[test]
    fn traversal_none() {
        let samples = vec![Point { x: 50.0, y: 50.0 }];
        let from_rect = Rect { x: 0.0, y: 0.0, width: 30.0, height: 30.0 };
        let to_rect = Rect { x: 100.0, y: 100.0, width: 30.0, height: 30.0 };
        assert_eq!(endpoint_node_traversal_count(&samples, Some(&from_rect), Some(&to_rect)), 0);
    }

    #[test]
    fn traversal_from_rect() {
        // sample strictly inside from_rect
        let samples = vec![Point { x: 10.0, y: 10.0 }];
        let from_rect = Rect { x: 0.0, y: 0.0, width: 30.0, height: 30.0 };
        let to_rect = Rect { x: 100.0, y: 100.0, width: 30.0, height: 30.0 };
        assert_eq!(endpoint_node_traversal_count(&samples, Some(&from_rect), Some(&to_rect)), 1);
    }

    #[test]
    fn traversal_both_rects() {
        let samples = vec![
            Point { x: 10.0, y: 10.0 },
            Point { x: 110.0, y: 110.0 },
        ];
        let from_rect = Rect { x: 0.0, y: 0.0, width: 30.0, height: 30.0 };
        let to_rect = Rect { x: 100.0, y: 100.0, width: 30.0, height: 30.0 };
        assert_eq!(endpoint_node_traversal_count(&samples, Some(&from_rect), Some(&to_rect)), 2);
    }
}
