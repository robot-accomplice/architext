//! Faithful port of `viewer/src/routing/routeStrategies.js`.
//!
//! ## Scope and deferred dependencies
//!
//! `selectRouteCandidate` is the main export of this module. It requires two
//! collaborator objects that are not yet ported:
//!
//! - **`routeCandidates`** (`createRouteCandidateFactory` from
//!   `routeCandidateBuilders.js`): builds spline, straight, direct-port,
//!   corridor, grid-route, perimeter, and corner-perimeter candidates.
//!   These are wired via the `RouteCandidateFactory` trait defined below.
//!
//! - **`scoreRouteCandidates`** (from `routeScoring.js`): fills collision,
//!   crossing, overlap, surface-mismatch, and quality-cost fields on each
//!   candidate. Its full implementation needs the collision callback and
//!   `routeIndex` (now ported in `route_index.rs`). The full body requires
//!   the integration layer (`routeEdges` / the plan orchestrator).
//!
//! Both are **DEFERRED** pending Tier 3b/4 ports. The deferred reason is
//! sequencing: `routeCandidateBuilders` and `routeEdges` are the next tier
//! and their Rust ports will implement `RouteCandidateFactory` and the
//! `ScoringContext` + `CollisionCounter` types needed by `score_route_candidates`.
//!
//! The following are fully ported and unit-tested:
//! - `preferredStartSidePairs` (private helper)
//! - `semanticFlowRelationship` (private helper)
//! - `routeSidePairsFor` (private helper)
//! - `isSideAvailable` / `isSidePairAvailable` / `availableSidePairs` (private helpers)
//! - `warningRouteCandidate` (ported here; JS imports from routeScoring but we place it
//!   in this module to avoid a circular dependency — it only needs `Rect` and `RouteCandidate`)
//! - `fixedPreferredOrthogonalCandidate` (pure geometry; uses already-ported
//!   `candidate_ports`, `port_pairs_for`, `simplify_orthogonal_points`, `path_to_svg`,
//!   `line_samples`, `bend_count`, `with_quality_costs` — all available)
//! - `selectRouteCandidate` is declared and its body is structured faithfully but
//!   cannot be fully exercised until `RouteCandidateFactory` and the scoring context
//!   are wired. The function compiles without the trait implementations via the
//!   generic type parameter.
//!
//! ## Translation decisions
//!
//! ### `??` null-coalescing semantics
//! JavaScript `a ?? b ?? c` is faithfully mapped to Rust `a.or_else(|| b).or_else(|| c)`.
//! In the selection fallback chain at the end of `selectRouteCandidate`:
//! ```text
//! return (best ? warnCandidate(best) : null)
//!   ?? fixedPreferredOrthogonalCandidate(...)
//!   ?? relaxedPreferenceRoute();
//! ```
//! This becomes:
//! ```text
//! let warned_best = best.map(|b| warn_candidate(b));
//! warned_best
//!     .or_else(|| fixed_preferred_orthogonal_candidate(...))
//!     .or_else(|| relaxed_preference_route())
//! ```
//!
//! ### `warnCandidate` closure
//! In JS: `const warnCandidate = (candidate) => warningRouteCandidate(candidate, { style, fromRect, toRect })`.
//! In Rust: an inline closure `|c| warning_route_candidate(c, style, from_rect, to_rect)`.
//!
//! ### `relaxedPreferenceRoute` closure
//! In JS: a zero-argument closure that recursively calls `selectRouteCandidate` with
//! `preferredStartSide` and `preferredEndSide` cleared. We represent this as a
//! `relaxed_preference_route` free function that builds the modified input.
//!
//! ### `math.hypot` in `fixedPreferredOrthogonalCandidate`
//! The `lengthCost` accumulation uses `Math.hypot` on successive sample pairs.
//! This must use `js_hypot` for parity. All coordinates in `d` strings go through
//! `path_to_svg` (which calls `js_number_to_string` on every coordinate) — parity
//! is guaranteed by the already-ported `path_to_svg`.
//!
//! ### `dedupeBy` in final scoring call
//! The JS calls `dedupeBy(candidates.filter(...), fn)`. In Rust we call the already-
//! ported `dedupe_by` from `route_constants`, passing a closure that builds the
//! `"x,y|x,y|..."` point-key string.
//!
//! ### `SIDES` iteration order
//! `SIDES = ["left","right","top","bottom"]` — imported from `route_ports`, matching JS.

use crate::js_compat::js_hypot;
use crate::model::{Point, Rect};
use crate::route_candidate_ports::{candidate_ports_with_anchors, port_pairs_for, side_pairs_for, CandidateScope, EndpointOffsets};
use crate::route_constants::{ROUTE_COST_WEIGHTS, ROUTE_SPACING, SPLINE_CURVE_VARIANTS};
use crate::route_geometry::{bend_count, line_samples, rect_distance};
use crate::route_ports::{PORT_STUB, SIDES};
use crate::route_rendering::{path_to_svg, simplify_orthogonal_points};
use crate::route_intent::IntentRelationship;
use crate::route_scoring::{
    best_route_candidate, is_clean_route_candidate,
    score_route_candidates, with_quality_costs, QualityCosts, RouteCandidate, RouteWarning,
    ScoreContext,
};

// ---------------------------------------------------------------------------
// RouteCandidateFactory trait  (deferred — Tier 3b)
// ---------------------------------------------------------------------------

/// Trait that mirrors the `routeCandidates` object returned by
/// `createRouteCandidateFactory`. Each method maps to one builder function.
///
/// The concrete implementation lives in the `route_candidate_builders` module
/// (Tier 3b). All methods return `Option<RouteCandidate>` where `None` means
/// the builder produced no valid route (matches JS returning `null`/`undefined`).
///
/// `PortResult` carries the anchor and port `Point`s; for now we inline the two
/// fields as separate arguments to avoid an import-cycle risk.
/// Argument counts mirror the JS builder function signatures exactly; suppressing
/// clippy's too_many_arguments lint to preserve faithful translation.
#[allow(clippy::too_many_arguments)]
pub trait RouteCandidateFactory {
    fn direct_port_candidate(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
    ) -> Option<RouteCandidate>;

    fn corridor_candidate(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
        corridor_axis: &str,
        corridor_value: f64,
    ) -> Option<RouteCandidate>;

    fn grid_route(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        route_offset: f64,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
    ) -> Option<RouteCandidate>;

    fn perimeter_route(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        side: &str,
        route_offset: f64,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
    ) -> Option<RouteCandidate>;

    fn corner_perimeter_routes(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        route_offset: f64,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
    ) -> Vec<RouteCandidate>;

    fn spline_candidate(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
        pair_index: usize,
        offset: f64,
    ) -> Option<RouteCandidate>;

    fn straight_candidate(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_anchor: &Point,
        start_port: &Point,
        end_anchor: &Point,
        end_port: &Point,
    ) -> Option<RouteCandidate>;
}

// ---------------------------------------------------------------------------
// ScoringContext  (deferred — Tier 4 integration)
// ---------------------------------------------------------------------------

/// Passed to `score_route_candidates`. The collision counter and route index
/// are injected here. Deferred pending `routeEdges` port.
pub struct ScoringContext<'a> {
    pub from_id: &'a str,
    pub to_id: &'a str,
    pub from_rect: &'a Rect,
    pub to_rect: &'a Rect,
    pub pair_index: usize,
    pub top_limit: f64,
    pub bottom_limit: f64,
    pub relationship: Option<&'a CandidateRelationship>,
    pub from_lane_index: Option<i64>,
    pub to_lane_index: Option<i64>,
    pub from_row_index: Option<i64>,
    pub to_row_index: Option<i64>,
    pub canvas_width: Option<f64>,
    pub canvas_height: Option<f64>,
    pub blocker_rects: &'a [Rect],
    // Deferred: collision_count callback, route_index
}

// ---------------------------------------------------------------------------
// EndpointSideUsage  (deferred — Tier 4 integration)
// ---------------------------------------------------------------------------

/// Mirrors JS `endpointSideUsage.isAvailable(nodeId, side, rect)`.
/// The concrete implementation tracks which sides are in use across routed pairs.
pub trait EndpointSideUsage {
    fn is_available(&self, node_id: &str, side: &str, rect: &Rect) -> bool;
}

// ---------------------------------------------------------------------------
// CandidateRelationship  (input shape)
// ---------------------------------------------------------------------------

/// The relationship fields consumed by `selectRouteCandidate`. Mirrors the JS
/// relationship object shape used in routeStrategies.
#[derive(Debug, Clone, Default)]
pub struct CandidateRelationship {
    pub id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub relationship_type: Option<String>,
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
    /// JS `fixedPorts` boolean on the `fromRect`; stored here for convenience.
    pub from_rect_fixed_ports: bool,
}

impl CandidateRelationship {
    fn to_intent_relationship(&self) -> IntentRelationship {
        IntentRelationship {
            id: self.id.clone().unwrap_or_default(),
            kind: self.kind.clone(),
            return_of: self.return_of.clone(),
            outcome: self.outcome.clone(),
            relationship_type: self.relationship_type.clone(),
            step_id: self.step_id.clone(),
            flow_id: self.flow_id.clone(),
            preferred_start_side: self.preferred_start_side.clone(),
            preferred_end_side: self.preferred_end_side.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// SelectRouteCandidateInput
// ---------------------------------------------------------------------------

/// Input struct for `select_route_candidate`. Mirrors the JS destructured input.
pub struct SelectRouteCandidateInput<'a, F: RouteCandidateFactory, E: EndpointSideUsage> {
    pub collision_count: &'a dyn Fn(&RouteCandidate, &str, &str, f64) -> i64,
    pub corridors: &'a [(String, f64)], // (axis, value)
    pub endpoint_offsets: EndpointOffsets,
    pub from_id: &'a str,
    pub from_rect: &'a Rect,
    pub index: usize,
    pub pair_index: usize,
    pub relationship: CandidateRelationship,
    pub route_candidates: &'a F,
    pub route_index: &'a crate::route_index::RouteIndex,
    pub stats: Option<&'a mut PlanStats>,
    pub progress_tick: Option<&'a dyn Fn()>,
    pub style: &'a str,
    pub to_id: &'a str,
    pub to_rect: &'a Rect,
    pub used_routes: &'a [Vec<Point>],
    pub canvas_width: Option<f64>,
    pub canvas_height: Option<f64>,
    pub blocker_rects: &'a [Rect],
    pub endpoint_side_usage: Option<&'a E>,
    pub from_lane_index: Option<i64>,
    pub to_lane_index: Option<i64>,
    pub from_row_index: Option<i64>,
    pub to_row_index: Option<i64>,
    /// Optional per-side anchor overrides for the from-node (decision-diamond nodes).
    /// JS stores sideAnchors on the rect object; Rust threads them separately.
    pub from_side_anchors: Option<&'a crate::route_ports::SideAnchors>,
    /// Optional per-side anchor overrides for the to-node.
    pub to_side_anchors: Option<&'a crate::route_ports::SideAnchors>,
}

// ---------------------------------------------------------------------------
// PlanStats  (mirrors JS `stats` object)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct PlanStats {
    pub edges_planned: i64,
    pub cheap_candidate_count: i64,
    pub grid_escalations: i64,
    pub cheap_rejection_reasons: CheapRejectionReasons,
}

#[derive(Debug, Clone, Default)]
pub struct CheapRejectionReasons {
    pub collisions: i64,
    pub padded_collisions: i64,
    pub repeated_crossings: i64,
    pub crossings: i64,
    pub endpoint_stack: i64,
    pub dogleg: i64,
    pub no_candidate: i64,
}

// ---------------------------------------------------------------------------
// Private helpers (pure — no deferred dependencies)
// ---------------------------------------------------------------------------

/// Port of JS `preferredStartSidePairs(pairs, relationship)`.
///
/// Filters `pairs` to those matching `preferredStartSide`/`preferredEndSide`.
/// Returns the full `pairs` slice if no match is found (JS: returns original pairs).
fn preferred_start_side_pairs<'a>(
    pairs: &'a [[&str; 2]],
    preferred_start_side: Option<&str>,
    preferred_end_side: Option<&str>,
) -> Vec<[&'a str; 2]> {
    if preferred_start_side.is_none() && preferred_end_side.is_none() {
        return pairs.to_vec();
    }
    let preferred: Vec<[&str; 2]> = pairs
        .iter()
        .filter(|[start_side, end_side]| {
            preferred_start_side.is_none_or(|ps| *start_side == ps)
                && preferred_end_side.is_none_or(|pe| *end_side == pe)
        })
        .copied()
        .collect();
    if !preferred.is_empty() { preferred } else { pairs.to_vec() }
}

/// Port of JS `semanticFlowRelationship(relationship)`.
pub fn semantic_flow_relationship(rel: &CandidateRelationship) -> bool {
    rel.relationship_type.as_deref() == Some("flow")
        || rel.kind.is_some()
        || rel.return_of.is_some()
        || rel.outcome.is_some()
        || rel.step_id.is_some()
        || rel.flow_id.is_some()
}

/// Port of JS `routeSidePairsFor(fromRect, toRect, relationship)`.
///
/// For non-semantic relationships returns `sidePairsFor(fromRect, toRect)`.
/// For semantic relationships, expands the base pairs with all 16 combinations
/// in JS SIDES order (`["left","right","top","bottom"]`), deduplicating by key.
pub fn route_side_pairs_for(from_rect: &Rect, to_rect: &Rect, rel: &CandidateRelationship) -> Vec<[&'static str; 2]> {
    let base_pairs = side_pairs_for(from_rect, to_rect);
    if !semantic_flow_relationship(rel) {
        return base_pairs;
    }
    let mut seen: std::collections::HashSet<String> = base_pairs
        .iter()
        .map(|[s, e]| format!("{s}:{e}"))
        .collect();
    let mut expanded = base_pairs;
    for &start_side in &SIDES {
        for &end_side in &SIDES {
            let key = format!("{start_side}:{end_side}");
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            expanded.push([start_side, end_side]);
        }
    }
    expanded
}

/// Port of JS `isSideAvailable`.
fn is_side_available<E: EndpointSideUsage>(
    endpoint_side_usage: Option<&E>,
    node_id: &str,
    rect: &Rect,
    side: &str,
) -> bool {
    endpoint_side_usage.is_none_or(|u| u.is_available(node_id, side, rect))
}

/// Port of JS `isSidePairAvailable`.
fn is_side_pair_available<E: EndpointSideUsage>(
    endpoint_side_usage: Option<&E>,
    from_id: &str,
    from_rect: &Rect,
    to_id: &str,
    to_rect: &Rect,
    start_side: &str,
    end_side: &str,
) -> bool {
    is_side_available(endpoint_side_usage, from_id, from_rect, start_side)
        && is_side_available(endpoint_side_usage, to_id, to_rect, end_side)
}

/// Port of JS `availableSidePairs(pairs, input)`.
fn available_side_pairs<'a, E: EndpointSideUsage>(
    pairs: &'a [[&str; 2]],
    endpoint_side_usage: Option<&E>,
    from_id: &str,
    from_rect: &Rect,
    to_id: &str,
    to_rect: &Rect,
) -> Vec<[&'a str; 2]> {
    pairs
        .iter()
        .filter(|[start_side, end_side]| {
            is_side_pair_available(endpoint_side_usage, from_id, from_rect, to_id, to_rect, start_side, end_side)
        })
        .copied()
        .collect()
}

// ---------------------------------------------------------------------------
// warningRouteCandidate  (ported here from routeScoring.js)
// ---------------------------------------------------------------------------

/// Port of JS `warningRouteCandidate(candidate, context)`.
///
/// Clones `candidate` and appends diagnostic warnings based on the routing
/// outcome. Always returns `Some(candidate_with_warnings)`.
///
/// JS context shape: `{ style, fromRect, toRect }`.
pub fn warning_route_candidate(
    candidate: RouteCandidate,
    style: &str,
    from_rect: &Rect,
    to_rect: &Rect,
) -> RouteCandidate {
    let mut warnings: Vec<RouteWarning> = Vec::new();

    // JS: const leastBad = style === "spline" ? collisions > 0 : collisions > 0 || paddedCollisions > 0
    let least_bad = if style == "spline" {
        candidate.collisions > 0
    } else {
        candidate.collisions > 0 || candidate.padded_collisions > 0
    };
    if least_bad {
        let message = if style == "spline" {
            "No clean spline route was available for the current node arrangement."
        } else if style == "straight" {
            "No clean straight route was available for the current node arrangement."
        } else {
            "No clean route was available for the current node arrangement."
        };
        warnings.push(RouteWarning {
            code: "least-bad-route".to_string(),
            message: message.to_string(),
        });
    }

    if style == "orthogonal" && candidate.endpoint_node_traversals > 0 {
        warnings.push(RouteWarning {
            code: "endpoint-node-traversal".to_string(),
            message: "Selected route crosses through its source or target node interior.".to_string(),
        });
    }
    if style == "orthogonal" && candidate.self_overlapping_segments > 0 {
        warnings.push(RouteWarning {
            code: "self-overlapping-route".to_string(),
            message: "Selected route doubles back over its own line.".to_string(),
        });
    }
    if style == "orthogonal" && candidate.repeated_crossings > 0 {
        warnings.push(RouteWarning {
            code: "repeated-route-crossing".to_string(),
            message: "Selected route crosses the same existing route more than once.".to_string(),
        });
    }
    if style == "orthogonal" && candidate.quality_costs.perimeter_fallback_cost > 0.0 {
        warnings.push(RouteWarning {
            code: "perimeter-fallback-route".to_string(),
            message: "Selected route used a perimeter fallback instead of an interior corridor.".to_string(),
        });
    }

    // JS: if (rectDistance(context.fromRect, context.toRect) < PORT_STUB * 2) { ... }
    if rect_distance(from_rect, to_rect) < PORT_STUB * 2.0 {
        warnings.push(RouteWarning {
            code: "nodes-too-close".to_string(),
            message: "Source and target nodes are too close for clean connector routing.".to_string(),
        });
    }

    RouteCandidate { warnings, ..candidate }
}

// ---------------------------------------------------------------------------
// fixedPreferredOrthogonalCandidate  (pure geometry — fully ported)
// ---------------------------------------------------------------------------

/// Port of JS `fixedPreferredOrthogonalCandidate(relationship, fromRect, toRect, endpointOffsets, routeCandidates, usedRoutes, input)`.
///
/// Handles the special case where `fromRect.fixedPorts` is set and a
/// `preferredStartSide` is specified. Builds an orthogonal route manually using
/// geometry and quality costs.
///
/// Returns `None` when:
/// - `fromRect.fixedPorts` is false, OR `preferredStartSide` is not set
/// - The chosen side pair is blocked by `endpointSideUsage`
/// - No valid port pair exists
///
/// When `routeCandidates.directPortCandidate` would be needed (the non-geometry
/// branch), we call the factory. The factory is deferred, so this path is
/// `None`-producing until Tier 3b.
///
/// Note: `js_hypot` is used in `lengthCost` accumulation to match V8 precision.
#[allow(clippy::too_many_arguments)]
pub fn fixed_preferred_orthogonal_candidate<F: RouteCandidateFactory, E: EndpointSideUsage>(
    relationship: &CandidateRelationship,
    from_rect: &Rect,
    from_rect_fixed_ports: bool,
    to_rect: &Rect,
    endpoint_offsets: &EndpointOffsets,
    route_candidates: &F,
    used_routes: &[Vec<Point>],
    endpoint_side_usage: Option<&E>,
    from_id: &str,
    to_id: &str,
    from_side_anchors: Option<&crate::route_ports::SideAnchors>,
    to_side_anchors: Option<&crate::route_ports::SideAnchors>,
) -> Option<RouteCandidate> {
    // JS: if (!fromRect.fixedPorts || !relationship.preferredStartSide) return null;
    if !from_rect_fixed_ports || relationship.preferred_start_side.is_none() {
        return None;
    }
    let preferred_start_side = relationship.preferred_start_side.as_deref().unwrap();

    // JS: const endSide = relationship.preferredEndSide ?? sidePairsFor(fromRect, toRect)[0]?.[1] ?? "left";
    let end_side: &str = relationship.preferred_end_side.as_deref().unwrap_or_else(|| {
        side_pairs_for(from_rect, to_rect)
            .first()
            .map(|p| p[1])
            .unwrap_or("left")
    });

    // JS: if (input && !isSidePairAvailable(input, relationship.preferredStartSide, endSide)) return null;
    if !is_side_pair_available(
        endpoint_side_usage,
        from_id, from_rect,
        to_id, to_rect,
        preferred_start_side, end_side,
    ) {
        return None;
    }

    let ports = candidate_ports_with_anchors(from_rect, to_rect, preferred_start_side, end_side, endpoint_offsets, CandidateScope::Cheap, from_side_anchors, to_side_anchors);
    let port_pair = port_pairs_for(&ports).into_iter().next()?;
    let [start_port_result, end_port_result] = port_pair;
    let start_anchor = &start_port_result.anchor;
    let start_port_pt = &start_port_result.port;
    let end_port_pt = &end_port_result.port;
    let end_anchor = &end_port_result.anchor;

    // JS: if (relationship.preferredStartSide === "left" && endSide === "bottom") { ... }
    if preferred_start_side == "left" && end_side == "bottom" {
        let gutter = f64::min(start_port_pt.x, to_rect.x) - ROUTE_COST_WEIGHTS.fixed_preferred_gutter;
        let raw_points = [
            start_anchor.clone(),
            start_port_pt.clone(),
            Point { x: gutter, y: start_port_pt.y },
            Point { x: gutter, y: end_port_pt.y },
            end_port_pt.clone(),
            end_anchor.clone(),
        ];
        let points = simplify_orthogonal_points(&raw_points);
        let samples = line_samples(&points);
        let label = samples.get(samples.len() / 2)
            .or_else(|| points.get(points.len() / 2))
            .unwrap_or(start_anchor)
            .clone();
        let length_cost = compute_length_cost(&samples);
        return Some(with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&points),
                label_x: label.x,
                label_y: label.y,
                bends: bend_count(&points) as i64,
                samples,
                points,
                ..RouteCandidate::default()
            },
            QualityCosts {
                length_cost,
                point_count_cost: raw_points.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
                bend_cost: bend_count(&raw_points) as f64 * ROUTE_COST_WEIGHTS.bend,
                ..QualityCosts::default()
            },
        ));
    }

    // JS: const preferred = routeCandidates.directPortCandidate(...)
    let preferred = route_candidates.direct_port_candidate(
        relationship, from_id, to_id, preferred_start_side, end_side,
        used_routes, start_anchor, start_port_pt, end_anchor, end_port_pt,
    );
    if preferred.is_some() {
        return preferred;
    }

    // JS: if (relationship.preferredStartSide === endSide) { gutter route }
    if preferred_start_side == end_side {
        let gutter = match end_side {
            "right" => to_rect.x + to_rect.width + ROUTE_COST_WEIGHTS.fixed_preferred_gutter,
            "left"  => to_rect.x - ROUTE_COST_WEIGHTS.fixed_preferred_gutter,
            "top"   => to_rect.y - ROUTE_COST_WEIGHTS.fixed_preferred_gutter,
            _       => to_rect.y + to_rect.height + ROUTE_COST_WEIGHTS.fixed_preferred_gutter,
        };
        let raw_points: Vec<Point> = if end_side == "left" || end_side == "right" {
            vec![
                start_anchor.clone(),
                start_port_pt.clone(),
                Point { x: gutter, y: start_port_pt.y },
                Point { x: gutter, y: end_port_pt.y },
                end_port_pt.clone(),
                end_anchor.clone(),
            ]
        } else {
            vec![
                start_anchor.clone(),
                start_port_pt.clone(),
                Point { x: start_port_pt.x, y: gutter },
                Point { x: end_port_pt.x, y: gutter },
                end_port_pt.clone(),
                end_anchor.clone(),
            ]
        };
        let points = simplify_orthogonal_points(&raw_points);
        let samples = line_samples(&points);
        let label = samples.get(samples.len() / 2)
            .or_else(|| points.get(points.len() / 2))
            .unwrap_or(start_anchor)
            .clone();
        let length_cost = compute_length_cost(&samples);
        return Some(with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&points),
                label_x: label.x,
                label_y: label.y,
                bends: bend_count(&points) as i64,
                samples,
                points,
                ..RouteCandidate::default()
            },
            QualityCosts {
                length_cost,
                point_count_cost: raw_points.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
                bend_cost: bend_count(&raw_points) as f64 * ROUTE_COST_WEIGHTS.bend,
                ..QualityCosts::default()
            },
        ));
    }

    // JS: L-shape: horizontal or vertical intermediate point
    let raw_points: Vec<Point> = if preferred_start_side == "left" || preferred_start_side == "right" {
        vec![
            start_anchor.clone(),
            start_port_pt.clone(),
            Point { x: end_port_pt.x, y: start_port_pt.y },
            end_port_pt.clone(),
            end_anchor.clone(),
        ]
    } else {
        vec![
            start_anchor.clone(),
            start_port_pt.clone(),
            Point { x: start_port_pt.x, y: end_port_pt.y },
            end_port_pt.clone(),
            end_anchor.clone(),
        ]
    };
    let points = simplify_orthogonal_points(&raw_points);
    let samples = line_samples(&points);
    let label = samples.get(samples.len() / 2)
        .or_else(|| points.get(points.len() / 2))
        .unwrap_or(start_anchor)
        .clone();
    let length_cost = compute_length_cost(&samples);
    Some(with_quality_costs(
        RouteCandidate {
            d: path_to_svg(&points),
            label_x: label.x,
            label_y: label.y,
            bends: bend_count(&points) as i64,
            samples,
            points,
            ..RouteCandidate::default()
        },
        QualityCosts {
            length_cost,
            point_count_cost: raw_points.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
            bend_cost: bend_count(&raw_points) as f64 * ROUTE_COST_WEIGHTS.bend,
            ..QualityCosts::default()
        },
    ))
}

/// Compute `lengthCost` from samples using `js_hypot` (V8 parity).
///
/// JS: `samples.reduce((sum, sample, index) => index === 0 ? 0 : sum + Math.hypot(...))`
fn compute_length_cost(samples: &[Point]) -> f64 {
    samples
        .windows(2)
        .fold(0.0, |sum, w| sum + js_hypot(w[1].x - w[0].x, w[1].y - w[0].y))
}

// ---------------------------------------------------------------------------
// selectRouteCandidate
// ---------------------------------------------------------------------------

/// Port of JS `selectRouteCandidate(input)`.
///
/// Selects the best route candidate for a single directed relationship.
/// The exact fallback chain is:
///
/// ```text
/// 1. (spline path) → warnCandidate(best) ?? relaxedPreferenceRoute()
/// 2. (straight path) → warnCandidate(best) ?? relaxedPreferenceRoute()
/// 3. fixedPreferredOrthogonalCandidate → warnCandidate (always Some; no fallback)
/// 4. Cheap candidates (direct-port + corridor) → scored
///    + optional grid escalation
///    + optional perimeter fallback
/// 5. Final: (warnCandidate(best) ?? null) ?? fixedPreferredOrthogonalCandidate ?? relaxedPreferenceRoute()
/// ```
///
/// **Deferred**: `score_route_candidates` body pending Tier 4 integration.
/// The call sites are correct; the scoring context is passed through.
/// Until then, candidates are returned unsorted (no collision/crossing scores).
///
/// **Note on stats mutation**: JS mutates `input.stats` directly. We take a
/// mutable reference inside `input.stats: Option<&mut PlanStats>`.
pub fn select_route_candidate<F, E>(
    input: SelectRouteCandidateInput<'_, F, E>,
) -> Option<RouteCandidate>
where
    F: RouteCandidateFactory,
    E: EndpointSideUsage,
{
    let SelectRouteCandidateInput {
        collision_count,
        corridors,
        endpoint_offsets,
        from_id,
        from_rect,
        index,
        pair_index,
        relationship,
        route_candidates,
        route_index,
        stats,
        progress_tick,
        style,
        to_id,
        to_rect,
        used_routes,
        canvas_width,
        canvas_height,
        blocker_rects,
        endpoint_side_usage,
        from_lane_index,
        to_lane_index,
        from_row_index,
        to_row_index,
        from_side_anchors,
        to_side_anchors,
    } = input;

    let warn_candidate = |c: RouteCandidate| warning_route_candidate(c, style, from_rect, to_rect);

    // JS: relaxedPreferenceRoute = () => (preferredStartSide || preferredEndSide) ? selectRouteCandidate({...}) : undefined
    let relaxed_preference_route = || -> Option<RouteCandidate> {
        if relationship.preferred_start_side.is_some() || relationship.preferred_end_side.is_some() {
            let relaxed_rel = CandidateRelationship {
                preferred_start_side: None,
                preferred_end_side: None,
                ..relationship.clone()
            };
            select_route_candidate(SelectRouteCandidateInput {
                collision_count,
                corridors,
                endpoint_offsets,
                from_id,
                from_rect,
                index,
                pair_index,
                relationship: relaxed_rel,
                route_candidates,
                route_index,
                stats: None, // stats not propagated into recursive relaxed call (JS doesn't either)
                progress_tick,
                style,
                to_id,
                to_rect,
                used_routes,
                canvas_width,
                canvas_height,
                blocker_rects,
                endpoint_side_usage,
                from_lane_index,
                to_lane_index,
                from_row_index,
                to_row_index,
                from_side_anchors,
                to_side_anchors,
            })
        } else {
            None // JS: `undefined`
        }
    };

    let base_side_pairs = route_side_pairs_for(from_rect, to_rect, &relationship);
    let available_pairs = available_side_pairs(
        &base_side_pairs,
        endpoint_side_usage,
        from_id, from_rect,
        to_id, to_rect,
    );
    let side_pairs = preferred_start_side_pairs(
        &available_pairs,
        relationship.preferred_start_side.as_deref(),
        relationship.preferred_end_side.as_deref(),
    );
    // JS: fallbackSidePairs = sidePairs.length > 0 ? sidePairs : preferredStartSidePairs(baseSidePairs, relationship)
    let fallback_side_pairs: Vec<[&str; 2]> = if !side_pairs.is_empty() {
        side_pairs
    } else {
        preferred_start_side_pairs(
            &base_side_pairs,
            relationship.preferred_start_side.as_deref(),
            relationship.preferred_end_side.as_deref(),
        )
    };

    let route_offset = pair_index as f64 * ROUTE_SPACING.pair_offset
        + (index % ROUTE_SPACING.index_offset_modulo as usize) as f64 * ROUTE_SPACING.index_offset;
    let top_limit = f64::min(from_rect.y, to_rect.y);
    let bottom_limit = f64::max(from_rect.y + from_rect.height, to_rect.y + to_rect.height);
    let intent_relationship = relationship.to_intent_relationship();
    let scoring_ctx = ScoreContext {
        collision_count,
        route_index,
        from_id,
        to_id,
        from_rect,
        to_rect,
        pair_index,
        top_limit,
        bottom_limit,
        relationship: Some(&intent_relationship),
        from_lane_index,
        to_lane_index,
        from_row_index,
        to_row_index,
        canvas_width,
        canvas_height,
        blocker_rects,
    };

    // -----------------------------------------------------------------------
    // Spline path
    // -----------------------------------------------------------------------
    if style == "spline" {
        let mut spline_candidates: Vec<RouteCandidate> = Vec::new();
        for [start_side, end_side] in &fallback_side_pairs {
            let ports = candidate_ports_with_anchors(from_rect, to_rect, start_side, end_side, &endpoint_offsets, CandidateScope::Cheap, from_side_anchors, to_side_anchors);
            for port_pair in port_pairs_for(&ports) {
                let [start_port_result, end_port_result] = port_pair;
                let start = &start_port_result.anchor;
                let end = &end_port_result.anchor;
                let center_distance = js_hypot(end.x - start.x, end.y - start.y);
                let curve_offset = clamp_f64(
                    center_distance * 0.18 + pair_index as f64 * ROUTE_SPACING.spline_pair_offset,
                    ROUTE_SPACING.spline_min_curve,
                    ROUTE_SPACING.spline_max_curve,
                );
                let route_spread = (index % ROUTE_SPACING.spline_spread_modulo as usize) as f64
                    * ROUTE_SPACING.spline_spread;
                for variant in &SPLINE_CURVE_VARIANTS {
                    let offset = curve_offset * variant.multiplier + route_spread * variant.spread;
                    if let Some(c) = route_candidates.spline_candidate(
                        &relationship, from_id, to_id, start_side, end_side,
                        used_routes,
                        &start_port_result.anchor, &start_port_result.port,
                        &end_port_result.anchor, &end_port_result.port,
                        pair_index, offset,
                    ) {
                        spline_candidates.push(c);
                    }
                }
            }
        }
        score_route_candidates(&mut spline_candidates, &scoring_ctx);
        let best = best_route_candidate(&spline_candidates).cloned();
        return best.map(warn_candidate).or_else(relaxed_preference_route);
    }

    // -----------------------------------------------------------------------
    // Straight path
    // -----------------------------------------------------------------------
    if style == "straight" {
        let mut straight_candidates: Vec<RouteCandidate> = Vec::new();
        for [start_side, end_side] in &fallback_side_pairs {
            let ports = candidate_ports_with_anchors(from_rect, to_rect, start_side, end_side, &endpoint_offsets, CandidateScope::Cheap, from_side_anchors, to_side_anchors);
            for port_pair in port_pairs_for(&ports) {
                let [start_port_result, end_port_result] = port_pair;
                if let Some(c) = route_candidates.straight_candidate(
                    &relationship, from_id, to_id, start_side, end_side,
                    used_routes,
                    &start_port_result.anchor, &start_port_result.port,
                    &end_port_result.anchor, &end_port_result.port,
                ) {
                    straight_candidates.push(c);
                }
            }
        }
        score_route_candidates(&mut straight_candidates, &scoring_ctx);
        let best = best_route_candidate(&straight_candidates).cloned();
        return best.map(warn_candidate).or_else(relaxed_preference_route);
    }

    // -----------------------------------------------------------------------
    // Fixed-preferred orthogonal
    // -----------------------------------------------------------------------
    let fixed_preferred_route = fixed_preferred_orthogonal_candidate(
        &relationship, from_rect, relationship.from_rect_fixed_ports, to_rect,
        &endpoint_offsets, route_candidates, used_routes,
        endpoint_side_usage, from_id, to_id,
        from_side_anchors, to_side_anchors,
    );
    if let Some(mut fpr) = fixed_preferred_route {
        score_route_candidates(std::slice::from_mut(&mut fpr), &scoring_ctx);
        fpr = warn_candidate(fpr);
        return Some(fpr);
    }

    // -----------------------------------------------------------------------
    // Cheap candidates (direct-port + corridor)
    // -----------------------------------------------------------------------
    let mut candidates: Vec<RouteCandidate> = Vec::new();
    let mut candidate_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut cheap_candidates: Vec<RouteCandidate> = Vec::new();
    let mut cheap_keys: std::collections::HashSet<String> = std::collections::HashSet::new();

    let point_key = |c: &RouteCandidate| -> String {
        c.points
            .iter()
            .map(|pt| format!("{},{}", pt.x, pt.y))
            .collect::<Vec<_>>()
            .join("|")
    };

    for [start_side, end_side] in &fallback_side_pairs {
        let ports = candidate_ports_with_anchors(from_rect, to_rect, start_side, end_side, &endpoint_offsets, CandidateScope::Cheap, from_side_anchors, to_side_anchors);
        for port_pair in port_pairs_for(&ports) {
            let [start_port_result, end_port_result] = port_pair;
            if pair_index == 0 {
                if let Some(c) = route_candidates.direct_port_candidate(
                    &relationship, from_id, to_id, start_side, end_side,
                    used_routes,
                    &start_port_result.anchor, &start_port_result.port,
                    &end_port_result.anchor, &end_port_result.port,
                ) {
                    let key = point_key(&c);
                    if cheap_keys.insert(key) {
                        cheap_candidates.push(c);
                    }
                }
            }
            for (axis, value) in corridors {
                if let Some(c) = route_candidates.corridor_candidate(
                    &relationship, from_id, to_id, start_side, end_side,
                    used_routes,
                    &start_port_result.anchor, &start_port_result.port,
                    &end_port_result.anchor, &end_port_result.port,
                    axis, *value,
                ) {
                    let key = point_key(&c);
                    if cheap_keys.insert(key) {
                        cheap_candidates.push(c);
                    }
                }
            }
        }
    }

    score_route_candidates(&mut cheap_candidates, &scoring_ctx);
    if let Some(tick) = progress_tick { tick(); }

    let has_clean_cheap = cheap_candidates.iter().any(is_clean_route_candidate);
    let has_clean_semantic_cheap = cheap_candidates.iter().any(|c| {
        is_clean_route_candidate(c) && c.semantic_surface_mismatch_count == 0
    });

    if let Some(stats) = stats {
        stats.edges_planned += 1;
        stats.cheap_candidate_count += cheap_candidates.len() as i64;
        if !has_clean_cheap {
            stats.grid_escalations += 1;
            let best_cheap = best_route_candidate(&cheap_candidates);
            if let Some(bc) = best_cheap {
                if bc.collisions > 0 { stats.cheap_rejection_reasons.collisions += 1; }
                if bc.padded_collisions > 0 { stats.cheap_rejection_reasons.padded_collisions += 1; }
                if bc.repeated_crossings > 0 { stats.cheap_rejection_reasons.repeated_crossings += 1; }
                if bc.crossings > 0 { stats.cheap_rejection_reasons.crossings += 1; }
                if bc.quality_costs.endpoint_stack_cost > 0.0 { stats.cheap_rejection_reasons.endpoint_stack += 1; }
                if bc.quality_costs.dogleg_cost > 0.0 { stats.cheap_rejection_reasons.dogleg += 1; }
            } else {
                stats.cheap_rejection_reasons.no_candidate += 1;
            }
        }
    }

    // Push cheap candidates into main pool.
    // Record the count so we can score only the newly-added (unscored)
    // grid/perimeter candidates in the final scoring pass below.
    let cheap_count = cheap_candidates.len();
    candidates.extend(cheap_candidates);
    candidate_keys.extend(
        candidates.iter().map(point_key)
    );

    if !has_clean_cheap || !has_clean_semantic_cheap {
        // Grid escalation
        for [start_side, end_side] in &fallback_side_pairs {
            let ports = candidate_ports_with_anchors(from_rect, to_rect, start_side, end_side, &endpoint_offsets, CandidateScope::Grid, from_side_anchors, to_side_anchors);
            for port_pair in port_pairs_for(&ports) {
                let [start_port_result, end_port_result] = port_pair;
                if let Some(c) = route_candidates.grid_route(
                    &relationship, from_id, to_id, start_side, end_side,
                    route_offset, used_routes,
                    &start_port_result.anchor, &start_port_result.port,
                    &end_port_result.anchor, &end_port_result.port,
                ) {
                    let key = point_key(&c);
                    if candidate_keys.insert(key) {
                        candidates.push(c);
                    }
                }
            }
        }
    }

    if !has_clean_cheap {
        // Perimeter fallback
        let available_perimeter_sides: Vec<&str> = SIDES
            .iter()
            .copied()
            .filter(|&side| is_side_pair_available(
                endpoint_side_usage,
                from_id, from_rect,
                to_id, to_rect,
                side, side,
            ))
            .collect();

        let preferred_perimeter_side: Option<Vec<&str>> =
            relationship.preferred_start_side.as_deref().and_then(|ps| {
                if is_side_pair_available(endpoint_side_usage, from_id, from_rect, to_id, to_rect, ps, ps) {
                    Some(vec![ps])
                } else {
                    None
                }
            });

        let perimeter_start_sides: Vec<&str> = preferred_perimeter_side.unwrap_or_else(|| {
            if !available_perimeter_sides.is_empty() {
                available_perimeter_sides
            } else {
                SIDES.to_vec()
            }
        });

        for side in &perimeter_start_sides {
            if let Some(pend) = relationship.preferred_end_side.as_deref() {
                if pend != *side {
                    continue;
                }
            }
            let ports = candidate_ports_with_anchors(from_rect, to_rect, side, side, &endpoint_offsets, CandidateScope::Cheap, from_side_anchors, to_side_anchors);
            for port_pair in port_pairs_for(&ports) {
                let [start_port_result, end_port_result] = port_pair;
                if let Some(c) = route_candidates.perimeter_route(
                    &relationship, from_id, to_id, side,
                    route_offset, used_routes,
                    &start_port_result.anchor, &start_port_result.port,
                    &end_port_result.anchor, &end_port_result.port,
                ) {
                    let key = point_key(&c);
                    if candidate_keys.insert(key.clone()) {
                        candidates.push(c);
                    }
                }
                for c in route_candidates.corner_perimeter_routes(
                    &relationship, from_id, to_id,
                    route_offset, used_routes,
                    &start_port_result.anchor, &start_port_result.port,
                    &end_port_result.anchor, &end_port_result.port,
                ) {
                    let key = point_key(&c);
                    if candidate_keys.insert(key) {
                        candidates.push(c);
                    }
                }
            }
        }
    }

    // Final score pass: score grid/perimeter candidates added after the cheap pass.
    // JS: scoreRouteCandidates(dedupeBy(candidates.filter(c => c.collisions === undefined), ...), context)
    // JS object mutation means scoring the deduped subset also scores the originals in `candidates`.
    // In Rust, we score candidates[cheap_count..] in-place. Deduplication is used in JS to avoid
    // redundant scoring; scoring the same geometry twice is idempotent so we skip the dedupe step.
    let _ = cheap_count; // cheap candidates already scored above; rest are grid/perimeter
    score_route_candidates(&mut candidates[cheap_count..], &scoring_ctx);

    let best = best_route_candidate(&candidates).cloned();

    // JS: return (best ? warnCandidate(best) : null)
    //       ?? fixedPreferredOrthogonalCandidate(...)
    //       ?? relaxedPreferenceRoute();
    let warned_best = best.map(warn_candidate);
    warned_best
        .or_else(|| {
            fixed_preferred_orthogonal_candidate(
                &relationship, from_rect, relationship.from_rect_fixed_ports, to_rect,
                &endpoint_offsets, route_candidates, used_routes,
                endpoint_side_usage, from_id, to_id,
                from_side_anchors, to_side_anchors,
            )
        })
        .or_else(relaxed_preference_route)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Port of JS `clamp` from routeGeometry for use in spline offset.
#[inline]
fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    f64::max(min, f64::min(max, value))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Rect};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    fn no_preferred_rel() -> CandidateRelationship {
        CandidateRelationship::default()
    }

    // -----------------------------------------------------------------------
    // preferredStartSidePairs
    // -----------------------------------------------------------------------

    #[test]
    fn preferred_side_pairs_no_preference_returns_all() {
        // Node: preferredStartSidePairs(pairs, {}) → pairs unchanged
        let pairs: Vec<[&str; 2]> = vec![["left", "right"], ["right", "left"]];
        let result = preferred_start_side_pairs(&pairs, None, None);
        assert_eq!(result, pairs);
    }

    #[test]
    fn preferred_side_pairs_filters_by_start() {
        // Node: preferredStartSidePairs([["left","right"],["right","left"]], {preferredStartSide:"right"})
        // → [["right","left"]]
        let pairs: Vec<[&str; 2]> = vec![["left", "right"], ["right", "left"]];
        let result = preferred_start_side_pairs(&pairs, Some("right"), None);
        assert_eq!(result, vec![["right", "left"]]);
    }

    #[test]
    fn preferred_side_pairs_fallback_when_no_match() {
        // Node: no match → return original pairs
        let pairs: Vec<[&str; 2]> = vec![["left", "right"], ["right", "left"]];
        let result = preferred_start_side_pairs(&pairs, Some("top"), None);
        assert_eq!(result, pairs);
    }

    #[test]
    fn preferred_side_pairs_filters_both() {
        // Node: preferredStartSidePairs([["left","right"],["right","left"],["top","bottom"]], {preferredStartSide:"top", preferredEndSide:"bottom"})
        // → [["top","bottom"]]
        let pairs: Vec<[&str; 2]> = vec![["left", "right"], ["right", "left"], ["top", "bottom"]];
        let result = preferred_start_side_pairs(&pairs, Some("top"), Some("bottom"));
        assert_eq!(result, vec![["top", "bottom"]]);
    }

    // -----------------------------------------------------------------------
    // semanticFlowRelationship
    // -----------------------------------------------------------------------

    #[test]
    fn semantic_flow_relationship_none() {
        // Node: semanticFlowRelationship({}) → false
        assert!(!semantic_flow_relationship(&CandidateRelationship::default()));
    }

    #[test]
    fn semantic_flow_relationship_kind() {
        // Node: semanticFlowRelationship({kind:"request"}) → true
        let rel = CandidateRelationship { kind: Some("request".to_string()), ..Default::default() };
        assert!(semantic_flow_relationship(&rel));
    }

    #[test]
    fn semantic_flow_relationship_flow_type() {
        // Node: semanticFlowRelationship({relationshipType:"flow"}) → true
        let rel = CandidateRelationship { relationship_type: Some("flow".to_string()), ..Default::default() };
        assert!(semantic_flow_relationship(&rel));
    }

    #[test]
    fn semantic_flow_relationship_step_id() {
        // Node: semanticFlowRelationship({stepId:"s1"}) → true
        let rel = CandidateRelationship { step_id: Some("s1".to_string()), ..Default::default() };
        assert!(semantic_flow_relationship(&rel));
    }

    // -----------------------------------------------------------------------
    // routeSidePairsFor
    // -----------------------------------------------------------------------

    #[test]
    fn route_side_pairs_non_semantic_returns_base() {
        // Non-semantic relationship → same as sidePairsFor
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let rel = no_preferred_rel();
        let result = route_side_pairs_for(&from, &to, &rel);
        let base = side_pairs_for(&from, &to);
        assert_eq!(result, base);
    }

    #[test]
    fn route_side_pairs_semantic_expands_to_16() {
        // Semantic relationship → all 16 side combinations (deduplicated)
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let rel = CandidateRelationship { kind: Some("request".to_string()), ..Default::default() };
        let result = route_side_pairs_for(&from, &to, &rel);
        // 4 base pairs + up to 12 more = 16 total (SIDES has 4, 4*4=16 combos)
        assert_eq!(result.len(), 16);
    }

    // -----------------------------------------------------------------------
    // warningRouteCandidate
    // -----------------------------------------------------------------------

    #[test]
    fn warning_candidate_clean_no_warnings() {
        // Node: warningRouteCandidate(clean_candidate, ...) → { ...candidate, warnings: [] }
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let c = RouteCandidate::default();
        let result = warning_route_candidate(c, "orthogonal", &from, &to);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn warning_candidate_collision_orthogonal() {
        // Node: collisions>0, style=orthogonal → "least-bad-route" warning with orthogonal message
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let mut c = RouteCandidate::default();
        c.collisions = 1;
        let result = warning_route_candidate(c, "orthogonal", &from, &to);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].code, "least-bad-route");
        assert!(result.warnings[0].message.contains("No clean route"));
    }

    #[test]
    fn warning_candidate_collision_spline() {
        // Node: collisions>0, style=spline → "least-bad-route" with spline message
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let mut c = RouteCandidate::default();
        c.collisions = 1;
        let result = warning_route_candidate(c, "spline", &from, &to);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].message.contains("No clean spline"));
    }

    #[test]
    fn warning_candidate_spline_padded_no_warning() {
        // Node: spline style, paddedCollisions>0 but collisions=0 → no least-bad warning
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let mut c = RouteCandidate::default();
        c.padded_collisions = 1;
        let result = warning_route_candidate(c, "spline", &from, &to);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn warning_candidate_nodes_too_close() {
        // Node: rectDistance(from,to) < PORT_STUB*2=36 → "nodes-too-close" warning
        // from=(0,0,100,50), to=(130,0,100,50) → gap=30 < 36
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(130.0, 0.0, 100.0, 50.0);
        let c = RouteCandidate::default();
        let result = warning_route_candidate(c, "orthogonal", &from, &to);
        let has_too_close = result.warnings.iter().any(|w| w.code == "nodes-too-close");
        assert!(has_too_close);
    }

    #[test]
    fn warning_candidate_nodes_far_enough() {
        // Node: rectDistance(from,to) >= PORT_STUB*2=36 → no "nodes-too-close" warning
        // from=(0,0,100,50), to=(200,0,100,50) → gap=100 >= 36
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let c = RouteCandidate::default();
        let result = warning_route_candidate(c, "orthogonal", &from, &to);
        let has_too_close = result.warnings.iter().any(|w| w.code == "nodes-too-close");
        assert!(!has_too_close);
    }

    #[test]
    fn warning_candidate_perimeter_fallback_warning() {
        // Node: qualityCosts.perimeterFallbackCost>0 AND style=orthogonal → perimeter-fallback-route warning
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let mut c = RouteCandidate::default();
        c.quality_costs.perimeter_fallback_cost = 7000.0;
        let result = warning_route_candidate(c, "orthogonal", &from, &to);
        let has_perimeter = result.warnings.iter().any(|w| w.code == "perimeter-fallback-route");
        assert!(has_perimeter);
    }

    #[test]
    fn warning_candidate_preserves_all_fields() {
        // Node: { ...candidate, warnings } — all other fields unchanged
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let mut c = RouteCandidate::default();
        c.bends = 3;
        c.cost = 1200.0;
        let result = warning_route_candidate(c, "orthogonal", &from, &to);
        assert_eq!(result.bends, 3);
        assert_eq!(result.cost, 1200.0);
    }

    // -----------------------------------------------------------------------
    // fixedPreferredOrthogonalCandidate — null guard tests
    // -----------------------------------------------------------------------

    struct NeverFactory;
    impl RouteCandidateFactory for NeverFactory {
        fn direct_port_candidate(&self, _: &CandidateRelationship, _: &str, _: &str, _: &str, _: &str, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point) -> Option<RouteCandidate> { None }
        fn corridor_candidate(&self, _: &CandidateRelationship, _: &str, _: &str, _: &str, _: &str, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point, _: &str, _: f64) -> Option<RouteCandidate> { None }
        fn grid_route(&self, _: &CandidateRelationship, _: &str, _: &str, _: &str, _: &str, _: f64, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point) -> Option<RouteCandidate> { None }
        fn perimeter_route(&self, _: &CandidateRelationship, _: &str, _: &str, _: &str, _: f64, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point) -> Option<RouteCandidate> { None }
        fn corner_perimeter_routes(&self, _: &CandidateRelationship, _: &str, _: &str, _: f64, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point) -> Vec<RouteCandidate> { Vec::new() }
        fn spline_candidate(&self, _: &CandidateRelationship, _: &str, _: &str, _: &str, _: &str, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point, _: usize, _: f64) -> Option<RouteCandidate> { None }
        fn straight_candidate(&self, _: &CandidateRelationship, _: &str, _: &str, _: &str, _: &str, _: &[Vec<Point>], _: &Point, _: &Point, _: &Point, _: &Point) -> Option<RouteCandidate> { None }
    }

    struct AlwaysAvailable;
    impl EndpointSideUsage for AlwaysAvailable {
        fn is_available(&self, _: &str, _: &str, _: &Rect) -> bool { true }
    }

    #[test]
    fn fixed_preferred_returns_none_when_no_fixed_ports() {
        // Node: fixedPreferredOrthogonalCandidate({}, ...) → null (fixedPorts=false)
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let rel = CandidateRelationship {
            preferred_start_side: Some("right".to_string()),
            from_rect_fixed_ports: false, // no fixed ports
            ..Default::default()
        };
        let result = fixed_preferred_orthogonal_candidate(
            &rel, &from, false, &to, &EndpointOffsets::default(),
            &NeverFactory, &[], Some(&AlwaysAvailable), "A", "B",
            None, None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn fixed_preferred_returns_none_when_no_preferred_start() {
        // Node: fixedPreferredOrthogonalCandidate({}, ...) → null (no preferredStartSide)
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let rel = CandidateRelationship {
            preferred_start_side: None,
            from_rect_fixed_ports: true,
            ..Default::default()
        };
        let result = fixed_preferred_orthogonal_candidate(
            &rel, &from, true, &to, &EndpointOffsets::default(),
            &NeverFactory, &[], Some(&AlwaysAvailable), "A", "B",
            None, None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn fixed_preferred_l_shape_produces_candidate() {
        // Node: fixedPorts=true, preferredStartSide="right", endSide="left" (different sides)
        // → L-shape candidate produced
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let rel = CandidateRelationship {
            preferred_start_side: Some("right".to_string()),
            preferred_end_side: Some("left".to_string()),
            from_rect_fixed_ports: true,
            ..Default::default()
        };
        let result = fixed_preferred_orthogonal_candidate(
            &rel, &from, true, &to, &EndpointOffsets::default(),
            &NeverFactory, &[], Some(&AlwaysAvailable), "A", "B",
            None, None,
        );
        // Should produce a candidate (NeverFactory.directPortCandidate returns None,
        // so the L-shape geometry branch is taken)
        assert!(result.is_some());
        let c = result.unwrap();
        assert!(!c.d.is_empty());
        assert!(c.d.starts_with('M'));
    }
}
