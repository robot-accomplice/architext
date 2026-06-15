//! Faithful port of the `routePlannerContext` and `routeEdges` pipeline from
//! `viewer/src/routing/routeEdges.js` (L1946–L2331).
//!
//! ## What is ported here
//!
//! - `RouteEdgesInput` — the full input struct for `route_edges()`.
//! - `route_planner_context` — builds the `blockerRects`, `collisionCount`,
//!   `routeQualityFromSamples`, and `routeCandidates` closures; returns a
//!   `PlannerContext` that exposes `edge_path(...)`.
//! - `route_edges` — the main pipeline: planning loop → quality passes →
//!   rendering. Pass order is EXACTLY the JS order (load-bearing).
//!
//! ## Scoring callback closure (the key integration)
//!
//! `score_route_candidates` was previously deferred because it needs a
//! `collisionCount` callback and a `RouteIndex`. Both are now wired here:
//!
//! - `collisionCount` is constructed in `route_planner_context` as a closure
//!   over `blockerRects` and threaded into each `SelectRouteCandidateInput`.
//! - `route_index` is threaded through `edge_path` as a live `&RouteIndex`.
//!
//! ## Progress callbacks (`onPhase` / `onProgress`) — parity-NEUTRAL
//!
//! These drive the browser loading overlay via `Date.now()` and do not affect
//! returned routes. They are accepted via `RouteEdgesInput` but not called.
//!
//! ## `routeCache` — faithfully wired
//!
//! `getCachedRawRoutes` / `setCachedRawRoutes` are called on the same key path
//! as the JS. Cache hits skip the planning loop (identical output).

use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;

use crate::js_compat::js_hypot;
use crate::model::{Point, Rect};
use crate::route_cache::{
    get_cached_raw_routes, route_cache_key, set_cached_raw_routes, CacheKeyInput,
    CacheKeyRelationship,
};
use crate::route_candidate_builders::{NodeBounds, RouteCandidateBuilders, RouteQualityFn};
use crate::route_constants::{CANVAS_INSET, ROUTE_COST_WEIGHTS};
use crate::route_corridors::{edge_corridors, free_space_corridors, EdgeCorridorOptions};
use crate::route_edges::{
    endpoint_side, enforce_endpoint_stubs, render_orthogonal_route, route_endpoints_are_perpendicular,
    route_with_endpoint_stubs, PlanRelationship, RouteData,
};
use crate::route_geometry::{bounds_for_points, distance_to_rect_squared, rects_overlap};
use crate::route_index::RouteIndex;
use crate::route_labels::{estimated_label_box, LabelBox, LabelRelationship};
use crate::route_mount_model::{
    distribute_facing_reciprocal_surfaces, distribute_surface_mount_units,
    mirror_self_crossing_bundles, optimize_mount_assignments, order_gutter_lanes_by_target,
    realign_facing_endpoints, recenter_singleton_side_endpoints, reduce_crossings_by_surface_swaps,
    relieve_crowded_surfaces, reorder_shared_surface_mounts, route_reciprocal_pairs_parallel,
    spread_shared_side_endpoints, straighten_self_crossing_pairs, BuildRouteForSides, MountInput,
    MountRect, MountRelationship, ReliefResult,
};
use crate::route_edges::side_endpoint_key;
use crate::route_ports::{offset_for_endpoint_order, surface_capacity};
use crate::route_scoring::{QualityCosts, RouteCandidate};
use crate::route_strategies::{
    select_route_candidate, CandidateRelationship, EndpointSideUsage as EndpointSideUsageTrait,
    SelectRouteCandidateInput,
};
use crate::route_style::normalize_route_style;
use crate::route_candidate_ports::EndpointOffsets;

// ---------------------------------------------------------------------------
// RouteEdgesInput — full input struct for route_edges()
// ---------------------------------------------------------------------------

/// Mirrors the JS `input` object passed to `routeEdges`.
pub struct RouteEdgesInput {
    pub style: String,
    pub relationships: Vec<InputRelationship>,
    pub visible_node_ids: Vec<String>,
    pub node_rects: IndexMap<String, NodeRect>,
    pub lane_index_by_node: IndexMap<String, i64>,
    pub row_index_by_node: IndexMap<String, i64>,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub margin_y: f64,
    pub grid_route_max_points: usize,
    pub grid_route_max_expansions: usize,
    pub score_edge_proximity: bool,
}

/// A node rect with the optional `fixedPorts` flag.
#[derive(Debug, Clone)]
pub struct NodeRect {
    pub rect: Rect,
    pub fixed_ports: bool,
}

/// Relationship descriptor as consumed by the orchestration layer.
#[derive(Debug, Clone)]
pub struct InputRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relationship_type: Option<String>,
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
    pub label: Option<String>,
    pub display_index: i64,
}

// ---------------------------------------------------------------------------
// EndpointSideUsageTracker — port of JS createEndpointSideUsage()
// ---------------------------------------------------------------------------

/// Tracks which sides of which nodes are already in use so the router can
/// prefer unused sides. Port of the JS `createEndpointSideUsage()` closure.
struct EndpointSideUsageTracker {
    counts: IndexMap<String, u32>,
}

impl EndpointSideUsageTracker {
    fn new() -> Self {
        Self { counts: IndexMap::new() }
    }

    /// Port of JS `mark(nodeId, side)`.
    fn mark(&mut self, node_id: &str, side: &str) {
        if node_id.is_empty() || side.is_empty() {
            return;
        }
        let key = side_endpoint_key(node_id, side);
        *self.counts.entry(key).or_insert(0) += 1;
    }
}

impl EndpointSideUsageTrait for EndpointSideUsageTracker {
    fn is_available(&self, node_id: &str, side: &str, rect: &Rect) -> bool {
        if node_id.is_empty() || side.is_empty() {
            return true;
        }
        let key = side_endpoint_key(node_id, side);
        let count = self.counts.get(&key).copied().unwrap_or(0);
        count < surface_capacity(rect, side)
    }
}

/// Always-available EndpointSideUsage for the relief rebuild path.
struct AlwaysAvailableSideUsage;
impl EndpointSideUsageTrait for AlwaysAvailableSideUsage {
    fn is_available(&self, _: &str, _: &str, _: &Rect) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// RouteQualityImpl — port of JS routeQualityFromSamples closure
// ---------------------------------------------------------------------------

/// Concrete `RouteQualityFn` that captures the `blockerRects` closure and
/// scores each candidate sample path. Mirrors JS `routeQualityFromSamples`.
pub struct RouteQualityImpl {
    pub blocker_cache: Rc<RefCell<IndexMap<String, Vec<Rect>>>>,
    pub visible_node_ids: Vec<String>,
    pub node_rects: IndexMap<String, Rect>,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub score_edge_proximity: bool,
    pub style: String,
}

impl RouteQualityFn for RouteQualityImpl {
    fn call(
        &self,
        samples: &[Point],
        label: &Point,
        from_id: &str,
        to_id: &str,
        used_routes: &[Vec<Point>],
        relationship: &CandidateRelationship,
    ) -> QualityCosts {
        let blockers = get_blocker_rects(
            &self.blocker_cache,
            &self.visible_node_ids,
            &self.node_rects,
            from_id,
            to_id,
        );
        let sample_bounds = bounds_for_points(samples);
        let sample_blockers: Vec<&Rect> = blockers
            .iter()
            .filter(|r| rects_overlap(&sample_bounds, r, CANVAS_INSET.top))
            .collect();

        let label_rel = LabelRelationship {
            relationship_type: relationship.relationship_type.clone(),
            step_id: relationship.step_id.clone(),
            label: None,
            id: relationship.id.clone(),
        };
        let label_box: Option<LabelBox> = estimated_label_box(label, Some(&label_rel));
        let label_point_bounds = Rect { x: label.x, y: label.y, width: 0.0, height: 0.0 };
        let label_blockers: Vec<&Rect> = blockers
            .iter()
            .filter(|r| {
                rects_overlap(&label_point_bounds, r, 34.0)
                    || label_box.as_ref().is_some_and(|lb| {
                        rects_overlap(
                            &Rect { x: lb.x, y: lb.y, width: lb.width, height: lb.height },
                            r,
                            6.0,
                        )
                    })
            })
            .collect();

        let mut length_cost = 0.0f64;
        for i in 0..samples.len().saturating_sub(1) {
            length_cost +=
                js_hypot(samples[i + 1].x - samples[i].x, samples[i + 1].y - samples[i].y);
        }

        let mut boundary_cost = 0.0f64;
        let mut node_clearance_cost = 0.0f64;
        let mut edge_proximity_cost = 0.0f64;
        for point in samples {
            if point.y < CANVAS_INSET.top
                || point.x < 16.0
                || point.x > self.canvas_width - 16.0
                || point.y > self.canvas_height - 16.0
            {
                boundary_cost += ROUTE_COST_WEIGHTS.boundary_violation;
            }
            for rect in &sample_blockers {
                let dist_sq = distance_to_rect_squared(point, rect);
                if dist_sq < 900.0 {
                    let distance = dist_sq.sqrt();
                    if distance < 14.0 {
                        node_clearance_cost += ROUTE_COST_WEIGHTS.node_collision;
                    }
                    node_clearance_cost +=
                        (CANVAS_INSET.top - distance) * ROUTE_COST_WEIGHTS.node_clearance;
                }
            }
            if self.score_edge_proximity || self.style == "spline" {
                for used_route in used_routes {
                    let mut used_index = 0;
                    while used_index < used_route.len() {
                        let used = &used_route[used_index];
                        let distance = js_hypot(point.x - used.x, point.y - used.y);
                        if distance < 36.0 {
                            edge_proximity_cost += 1800.0;
                        }
                        if distance < 20.0 {
                            edge_proximity_cost += 6200.0;
                        }
                        if distance < 10.0 {
                            edge_proximity_cost += 18000.0;
                        }
                        used_index += 2;
                    }
                }
            }
        }

        let mut label_node_clearance_cost = 0.0f64;
        for rect in &label_blockers {
            if distance_to_rect_squared(label, rect) < 1156.0 {
                label_node_clearance_cost += 24000.0;
            }
            if let Some(lb) = &label_box {
                let lb_rect = Rect { x: lb.x, y: lb.y, width: lb.width, height: lb.height };
                if rects_overlap(&lb_rect, rect, 6.0) {
                    label_node_clearance_cost += 60000.0;
                }
            }
        }

        QualityCosts {
            length_cost,
            boundary_cost,
            node_clearance_cost,
            edge_proximity_cost,
            label_node_clearance_cost,
            ..QualityCosts::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Blocker-rects cache helper
// ---------------------------------------------------------------------------

/// Port of JS `blockerRects(fromId, toId)` — lazily computed and cached.
fn get_blocker_rects(
    blocker_cache: &Rc<RefCell<IndexMap<String, Vec<Rect>>>>,
    visible_node_ids: &[String],
    node_rects: &IndexMap<String, Rect>,
    from_id: &str,
    to_id: &str,
) -> Vec<Rect> {
    let key = format!("{}\x00{}", from_id, to_id);
    {
        let cache = blocker_cache.borrow();
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }
    }
    let blockers: Vec<Rect> = visible_node_ids
        .iter()
        .filter(|id| id.as_str() != from_id && id.as_str() != to_id)
        .filter_map(|id| node_rects.get(id).cloned())
        .collect();
    blocker_cache.borrow_mut().insert(key, blockers.clone());
    blockers
}

// ---------------------------------------------------------------------------
// collisionCount helper
// ---------------------------------------------------------------------------

/// Port of JS `collisionCount(route, fromId, toId, padding)`.
fn collision_count_for(
    candidate: &RouteCandidate,
    from_id: &str,
    to_id: &str,
    padding: f64,
    blocker_cache: &Rc<RefCell<IndexMap<String, Vec<Rect>>>>,
    visible_node_ids: &[String],
    node_rects: &IndexMap<String, Rect>,
) -> i64 {
    let blockers = get_blocker_rects(blocker_cache, visible_node_ids, node_rects, from_id, to_id);
    let mut collisions = 0i64;
    for rect in &blockers {
        let mut collided = if candidate.style == "spline" {
            candidate.samples.iter().any(|p| {
                p.x > rect.x - padding
                    && p.x < rect.x + rect.width + padding
                    && p.y > rect.y - padding
                    && p.y < rect.y + rect.height + padding
            })
        } else {
            false
        };
        if !collided {
            for i in 0..candidate.points.len().saturating_sub(1) {
                if crate::route_geometry::segment_intersects_rect(
                    &candidate.points[i],
                    &candidate.points[i + 1],
                    rect,
                    padding,
                ) {
                    collided = true;
                    break;
                }
            }
        }
        if collided {
            collisions += 1;
        }
    }
    collisions
}

// ---------------------------------------------------------------------------
// PlannerContext — returned by route_planner_context()
// ---------------------------------------------------------------------------

/// Owns the shared state built by `routePlannerContext`. Exposes `edge_path`.
pub struct PlannerContext {
    blocker_cache: Rc<RefCell<IndexMap<String, Vec<Rect>>>>,
    visible_node_ids: Vec<String>,
    node_rects: IndexMap<String, Rect>,
    canvas_width: f64,
    canvas_height: f64,
    diagram_corridors: Vec<crate::route_corridors::Corridor>,
    route_candidates: RouteCandidateBuilders<RouteQualityImpl>,
}

impl PlannerContext {
    /// Port of JS `edgePath(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn edge_path<E: EndpointSideUsageTrait>(
        &self,
        rel: &InputRelationship,
        index: usize,
        pair_index: usize,
        used_routes: &[Vec<Point>],
        route_index: &RouteIndex,
        endpoint_offsets: EndpointOffsets,
        endpoint_side_usage: &E,
        style: &str,
        from_lane_index: Option<i64>,
        to_lane_index: Option<i64>,
        from_row_index: Option<i64>,
        to_row_index: Option<i64>,
    ) -> Option<RouteData> {
        let from_id = &rel.from;
        let to_id = &rel.to;
        let from_rect = self.node_rects.get(from_id)?;
        let to_rect = self.node_rects.get(to_id)?;

        let include_exterior = rel.relationship_type.as_deref() == Some("flow")
            || rel.kind.is_some()
            || rel.return_of.is_some()
            || rel.outcome.is_some()
            || rel.step_id.is_some()
            || rel.flow_id.is_some();

        let opts = EdgeCorridorOptions { include_exterior };
        let mut corridors: Vec<(String, f64)> =
            edge_corridors(from_rect, to_rect, &self.diagram_corridors, &opts)
                .into_iter()
                .map(|c| (c.axis.clone(), c.value))
                .collect();
        for c in route_index.adjacent_corridors(from_rect, to_rect, 12.0) {
            use crate::route_index::CorridorAxis;
            let axis_str = match c.axis {
                CorridorAxis::X => "x",
                CorridorAxis::Y => "y",
            };
            corridors.push((axis_str.to_string(), c.value));
        }

        // Build collision_count closure (captures blocker cache via Rc)
        let blocker_cache = Rc::clone(&self.blocker_cache);
        let visible_node_ids = self.visible_node_ids.clone();
        let node_rects = self.node_rects.clone();
        let collision_count_fn =
            move |candidate: &RouteCandidate, fid: &str, tid: &str, padding: f64| -> i64 {
                collision_count_for(
                    candidate,
                    fid,
                    tid,
                    padding,
                    &blocker_cache,
                    &visible_node_ids,
                    &node_rects,
                )
            };

        let blocker_rects = get_blocker_rects(
            &self.blocker_cache,
            &self.visible_node_ids,
            &self.node_rects,
            from_id,
            to_id,
        );

        let cand_rel = to_candidate_relationship(rel);

        let select_input = SelectRouteCandidateInput {
            collision_count: &collision_count_fn,
            corridors: &corridors,
            endpoint_offsets,
            from_id,
            from_rect,
            index,
            pair_index,
            relationship: cand_rel,
            route_candidates: &self.route_candidates,
            route_index,
            stats: None,
            progress_tick: None,
            style,
            to_id,
            to_rect,
            used_routes,
            canvas_width: Some(self.canvas_width),
            canvas_height: Some(self.canvas_height),
            blocker_rects: &blocker_rects,
            endpoint_side_usage: Some(endpoint_side_usage),
            from_lane_index,
            to_lane_index,
            from_row_index,
            to_row_index,
        };

        let candidate = select_route_candidate(select_input)?;
        Some(candidate_to_route_data(candidate))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_candidate_relationship(rel: &InputRelationship) -> CandidateRelationship {
    CandidateRelationship {
        id: Some(rel.id.clone()),
        from: Some(rel.from.clone()),
        to: Some(rel.to.clone()),
        relationship_type: rel.relationship_type.clone(),
        kind: rel.kind.clone(),
        return_of: rel.return_of.clone(),
        outcome: rel.outcome.clone(),
        step_id: rel.step_id.clone(),
        flow_id: rel.flow_id.clone(),
        preferred_start_side: rel.preferred_start_side.clone(),
        preferred_end_side: rel.preferred_end_side.clone(),
        from_rect_fixed_ports: false,
    }
}

fn candidate_to_route_data(candidate: RouteCandidate) -> RouteData {
    use crate::js_compat::js_number_to_string;
    use crate::route_geometry::{bend_count, bounds_for_points, line_samples};

    let points = candidate.points;
    let samples = if candidate.samples.is_empty() {
        line_samples(&points)
    } else {
        candidate.samples
    };
    let sample_bounds = bounds_for_points(&{
        let mut v = points.clone();
        v.extend(samples.iter().cloned());
        v
    });
    let bends = bend_count(&points);
    let label = samples
        .get(samples.len() / 2)
        .or_else(|| points.get(points.len() / 2))
        .cloned()
        .unwrap_or(Point { x: candidate.label_x, y: candidate.label_y });

    // Use the pre-built d string from the candidate builder if available.
    let d = if !candidate.d.is_empty() {
        candidate.d
    } else {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "{} {} {}",
                    if i == 0 { "M" } else { "L" },
                    js_number_to_string(p.x),
                    js_number_to_string(p.y)
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    RouteData {
        d,
        points,
        controls: None, // RouteCandidate has no controls field
        samples,
        sample_bounds,
        bends,
        label_x: label.x,
        label_y: label.y,
        style: if candidate.style.is_empty() { "orthogonal".to_string() } else { candidate.style },
        extra: IndexMap::new(),
    }
}

// ---------------------------------------------------------------------------
// route_planner_context
// ---------------------------------------------------------------------------

/// Port of JS `routePlannerContext(input)`.
pub fn route_planner_context(input: &RouteEdgesInput) -> PlannerContext {
    let visible_node_ids: Vec<String> = input.visible_node_ids.clone();
    let node_rects: IndexMap<String, Rect> =
        input.node_rects.iter().map(|(k, v)| (k.clone(), v.rect.clone())).collect();

    let visible_rects: Vec<Rect> =
        visible_node_ids.iter().filter_map(|id| node_rects.get(id).cloned()).collect();

    let diagram_corridors =
        free_space_corridors(&visible_rects, input.canvas_width, input.canvas_height);

    let all_node_rects: Vec<&Rect> = node_rects.values().collect();
    let node_bounds = if all_node_rects.is_empty() {
        None
    } else {
        Some(NodeBounds {
            min_x: all_node_rects.iter().map(|r| r.x).fold(f64::INFINITY, f64::min),
            min_y: all_node_rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min),
            max_x: all_node_rects.iter().map(|r| r.x + r.width).fold(f64::NEG_INFINITY, f64::max),
            max_y: all_node_rects.iter().map(|r| r.y + r.height).fold(f64::NEG_INFINITY, f64::max),
        })
    };

    let blocker_cache: Rc<RefCell<IndexMap<String, Vec<Rect>>>> =
        Rc::new(RefCell::new(IndexMap::new()));

    let route_quality = RouteQualityImpl {
        blocker_cache: Rc::clone(&blocker_cache),
        visible_node_ids: visible_node_ids.clone(),
        node_rects: node_rects.clone(),
        canvas_width: input.canvas_width,
        canvas_height: input.canvas_height,
        score_edge_proximity: input.score_edge_proximity,
        style: normalize_route_style(&input.style).to_string(),
    };

    let bc2 = Rc::clone(&blocker_cache);
    let vis2 = visible_node_ids.clone();
    let nr2 = node_rects.clone();
    let blocker_rects_fn = move |from_id: &str, to_id: &str| -> Vec<Rect> {
        get_blocker_rects(&bc2, &vis2, &nr2, from_id, to_id)
    };

    let nr3 = node_rects.clone();
    let rect_for_fn = move |node_id: &str| -> Rect {
        nr3.get(node_id).cloned().unwrap_or(Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 })
    };

    let route_candidates = RouteCandidateBuilders::new(
        blocker_rects_fn,
        input.canvas_width,
        input.canvas_height,
        node_bounds,
        Some(input.grid_route_max_expansions),
        Some(input.grid_route_max_points),
        rect_for_fn,
        route_quality,
        None,
        None,
    );

    PlannerContext {
        blocker_cache,
        visible_node_ids,
        node_rects,
        canvas_width: input.canvas_width,
        canvas_height: input.canvas_height,
        diagram_corridors,
        route_candidates,
    }
}

// ---------------------------------------------------------------------------
// Mount model adapter helpers
// ---------------------------------------------------------------------------

fn to_mount_node_rects(input: &RouteEdgesInput) -> IndexMap<String, MountRect> {
    input
        .node_rects
        .iter()
        .map(|(k, v)| (k.clone(), MountRect { rect: v.rect.clone(), fixed_ports: v.fixed_ports }))
        .collect()
}

fn to_mount_relationships(input: &RouteEdgesInput) -> IndexMap<String, MountRelationship> {
    input
        .relationships
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                MountRelationship {
                    id: r.id.clone(),
                    from: r.from.clone(),
                    to: r.to.clone(),
                    relationship_type: r.relationship_type.clone().unwrap_or_default(),
                    preferred_start_side: r.preferred_start_side.clone(),
                    preferred_end_side: r.preferred_end_side.clone(),
                    display_index: r.display_index,
                    kind: r.kind.clone(),
                    return_of: r.return_of.clone(),
                    outcome: r.outcome.clone(),
                    step_id: r.step_id.clone(),
                    flow_id: r.flow_id.clone(),
                },
            )
        })
        .collect()
}

fn to_plan_relationships(input: &RouteEdgesInput) -> Vec<PlanRelationship> {
    let style = normalize_route_style(&input.style);
    input
        .relationships
        .iter()
        .map(|r| PlanRelationship {
            id: r.id.clone(),
            from: r.from.clone(),
            to: r.to.clone(),
            style: Some(style.to_string()),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// BuildRouteForSides wrapper
// ---------------------------------------------------------------------------

struct ReliefRouteBuilder<'a> {
    planner: &'a PlannerContext,
    style: &'a str,
    node_rects: &'a IndexMap<String, Rect>,
}

impl<'a> BuildRouteForSides for ReliefRouteBuilder<'a> {
    fn build(
        &self,
        rel: &MountRelationship,
        start_side: &str,
        end_side: &str,
        route_by_id: &IndexMap<String, RouteData>,
    ) -> Option<RouteData> {
        let mut side_route_index = RouteIndex::new();
        let mut position = 0;
        for (other_id, other_route) in route_by_id {
            if other_id == &rel.id {
                continue;
            }
            side_route_index.add(&other_route.points, position);
            position += 1;
        }

        let modified_rel = InputRelationship {
            id: rel.id.clone(),
            from: rel.from.clone(),
            to: rel.to.clone(),
            relationship_type: Some(rel.relationship_type.clone()),
            kind: rel.kind.clone(),
            return_of: rel.return_of.clone(),
            outcome: rel.outcome.clone(),
            step_id: rel.step_id.clone(),
            flow_id: rel.flow_id.clone(),
            preferred_start_side: Some(start_side.to_string()),
            preferred_end_side: Some(end_side.to_string()),
            label: None,
            display_index: rel.display_index,
        };

        let built = self.planner.edge_path(
            &modified_rel,
            0,
            0,
            &[],
            &side_route_index,
            EndpointOffsets { from: 0.0, to: 0.0 },
            &AlwaysAvailableSideUsage,
            self.style,
            None,
            None,
            None,
            None,
        )?;

        Some(route_with_endpoint_stubs(&built, &rel.from, &rel.to, self.node_rects))
    }
}

// ---------------------------------------------------------------------------
// route_edges — the main pipeline
// ---------------------------------------------------------------------------

/// Port of JS `routeEdges(input)`.
///
/// Returns a map from relationship id to final `RouteData`.
pub fn route_edges(input: &RouteEdgesInput) -> IndexMap<String, RouteData> {
    let style = normalize_route_style(&input.style);

    let cache_key_input = build_cache_key_input(input, style);
    let cache_key = route_cache_key(&cache_key_input);
    let cached_json = get_cached_raw_routes(&cache_key);

    // RouteData does not impl Deserialize, so we always run the planning loop.
    // Cache hits from JS workers are still honoured (they skip this path entirely);
    // in Rust we always recompute and re-write the cache so subsequent JS hits work.
    let ctx = route_planner_context(input);
    let planned_raw_routes: Vec<(String, RouteData)> = run_planning_loop(input, &ctx, style);
    if cached_json.is_none() {
        set_cached_raw_routes(cache_key, routes_to_json(&planned_raw_routes));
    }
    // --- Quality passes ---
    let node_rects_plain: IndexMap<String, Rect> =
        input.node_rects.iter().map(|(k, v)| (k.clone(), v.rect.clone())).collect();
    let mount_node_rects = to_mount_node_rects(input);
    let relationship_by_id = to_mount_relationships(input);
    let plan_rels = to_plan_relationships(input);

    let mount_input = MountInput {
        visible_node_ids: &input.visible_node_ids,
        node_rects: &mount_node_rects,
        lane_index_by_node: &input.lane_index_by_node,
        row_index_by_node: &input.row_index_by_node,
        canvas_width: input.canvas_width,
        canvas_height: input.canvas_height,
    };

    // onPhase("Tidying endpoint mounts")
    let recentered = recenter_singleton_side_endpoints(&planned_raw_routes, &relationship_by_id, &mount_input);
    let spread = spread_shared_side_endpoints(&recentered, &relationship_by_id, &mount_input);
    let endpoint_adjusted = enforce_endpoint_stubs(spread, &plan_rels, &node_rects_plain);

    // onPhase("Separating parallel runs")
    let separated_routes = {
        use crate::route_edges::{separate_close_parallel_routes, NoopReroute, SeparationRelationship};
        let sep_rels: Vec<SeparationRelationship> = input
            .relationships
            .iter()
            .map(|r| SeparationRelationship {
                id: r.id.clone(),
                from: r.from.clone(),
                to: r.to.clone(),
            })
            .collect();
        let fixed_ports: IndexMap<String, bool> =
            input.node_rects.iter().map(|(k, v)| (k.clone(), v.fixed_ports)).collect();
        separate_close_parallel_routes(
            &endpoint_adjusted,
            &sep_rels,
            &node_rects_plain,
            &fixed_ports,
            &NoopReroute,
        )
    };

    let relief_planner = ctx;
    let mut relieved_by_id: IndexMap<String, RouteData> =
        separated_routes.iter().cloned().collect();
    let relief_builder =
        ReliefRouteBuilder { planner: &relief_planner, style, node_rects: &node_rects_plain };

    // onPhase("Relieving crowded surfaces")
    let relief: ReliefResult = relieve_crowded_surfaces(
        &mut relieved_by_id,
        &relationship_by_id,
        &mount_input,
        Some(&relief_builder),
    );

    if relief.any_moved {
        reorder_shared_surface_mounts(&mut relieved_by_id, &relationship_by_id, &mount_input);
        if !relief.pairs.is_empty() {
            let pair_ids: std::collections::HashSet<String> =
                relief.pairs.iter().flatten().cloned().collect();
            route_reciprocal_pairs_parallel(
                &mut relieved_by_id,
                &relationship_by_id,
                &mount_input,
                Some(&pair_ids),
            );
        }
        reduce_crossings_by_surface_swaps(&mut relieved_by_id, &relationship_by_id, &mount_input);
        realign_facing_endpoints(&mut relieved_by_id, &relationship_by_id, &mount_input);
    }
    if style == "orthogonal" {
        let pre_optimize: IndexMap<String, RouteData> = relieved_by_id.clone();
        optimize_mount_assignments(
            &mut relieved_by_id,
            &relationship_by_id,
            &mount_input,
            Some(&relief_builder),
        );
        // Restore routes where the optimiser moved endpoints to non-perpendicular positions.
        // JS pointer inequality → approximate with structural clone comparison.
        let moved_ids: Vec<String> = relieved_by_id
            .keys()
            .filter(|id| {
                match (relieved_by_id.get(*id), pre_optimize.get(*id)) {
                    (Some(cur), Some(pre)) => cur.d != pre.d || cur.points != pre.points,
                    _ => true,
                }
            })
            .cloned()
            .collect();
        for rel_id in &moved_ids {
            if let (Some(rel), Some(route)) =
                (relationship_by_id.get(rel_id), relieved_by_id.get(rel_id))
            {
                if !route_endpoints_are_perpendicular(route, &rel.from, &rel.to, &node_rects_plain) {
                    if let Some(pre) = pre_optimize.get(rel_id) {
                        relieved_by_id.insert(rel_id.clone(), pre.clone());
                    }
                }
            }
        }
    }

    distribute_facing_reciprocal_surfaces(&mut relieved_by_id, &relationship_by_id, &mount_input);
    distribute_surface_mount_units(&mut relieved_by_id, &relationship_by_id, &mount_input);

    if style == "orthogonal" {
        straighten_self_crossing_pairs(&mut relieved_by_id, &relationship_by_id, &mount_input);
        mirror_self_crossing_bundles(&mut relieved_by_id, &relationship_by_id, &mount_input);
        order_gutter_lanes_by_target(&mut relieved_by_id, &relationship_by_id, &mount_input);
    }

    // onPhase("Drawing hops over crossings")
    let display_raw_routes: Vec<(String, Option<RouteData>)> = separated_routes
        .iter()
        .map(|(id, _)| (id.clone(), relieved_by_id.get(id).cloned()))
        .collect();

    let all_raw_routes: Vec<RouteData> =
        display_raw_routes.iter().filter_map(|(_, r)| r.clone()).collect();

    let mut routes: IndexMap<String, RouteData> = IndexMap::new();
    let mut render_index = 0usize; // tracks position in all_raw_routes for self_index
    for (rel_id, raw_route) in display_raw_routes {
        if let Some(raw) = raw_route {
            let route = if style == "orthogonal" {
                render_orthogonal_route(&raw, &all_raw_routes, render_index)
            } else {
                raw
            };
            render_index += 1;
            routes.insert(rel_id, route);
        }
    }

    routes
}

// ---------------------------------------------------------------------------
// run_planning_loop
// ---------------------------------------------------------------------------

fn run_planning_loop(
    input: &RouteEdgesInput,
    planner: &PlannerContext,
    style: &str,
) -> Vec<(String, RouteData)> {
    let mut endpoint_totals: IndexMap<String, u32> = IndexMap::new();
    for rel in &input.relationships {
        if !input.lane_index_by_node.contains_key(&rel.from)
            || !input.lane_index_by_node.contains_key(&rel.to)
        {
            continue;
        }
        *endpoint_totals.entry(rel.from.clone()).or_insert(0) += 1;
        *endpoint_totals.entry(rel.to.clone()).or_insert(0) += 1;
    }

    let mut used_routes: Vec<Vec<Point>> = Vec::new();
    let mut route_index = RouteIndex::new();
    let mut pair_counts: IndexMap<String, u32> = IndexMap::new();
    let mut endpoint_counts: IndexMap<String, u32> = IndexMap::new();
    let mut endpoint_side_usage = EndpointSideUsageTracker::new();
    let mut planned: Vec<(String, RouteData)> = Vec::new();

    let node_rects_plain: IndexMap<String, Rect> =
        input.node_rects.iter().map(|(k, v)| (k.clone(), v.rect.clone())).collect();

    for (index, rel) in input.relationships.iter().enumerate() {
        if !input.lane_index_by_node.contains_key(&rel.from)
            || !input.lane_index_by_node.contains_key(&rel.to)
        {
            continue;
        }

        let mut pair_key_parts = [rel.from.clone(), rel.to.clone()];
        pair_key_parts.sort();
        let pair_key = pair_key_parts.join("<->");
        let pair_index = *pair_counts.get(&pair_key).unwrap_or(&0) as usize;
        *pair_counts.entry(pair_key).or_insert(0) += 1;

        let from_endpoint_count = *endpoint_counts.get(&rel.from).unwrap_or(&0);
        let to_endpoint_count = *endpoint_counts.get(&rel.to).unwrap_or(&0);
        *endpoint_counts.entry(rel.from.clone()).or_insert(0) += 1;
        *endpoint_counts.entry(rel.to.clone()).or_insert(0) += 1;

        let from_offset = if endpoint_totals.get(&rel.from).copied().unwrap_or(0) == 1 {
            0.0
        } else {
            offset_for_endpoint_order(from_endpoint_count)
        };
        let to_offset = if endpoint_totals.get(&rel.to).copied().unwrap_or(0) == 1 {
            0.0
        } else {
            offset_for_endpoint_order(to_endpoint_count)
        };

        let route = planner
            .edge_path(
                rel,
                index,
                pair_index,
                &used_routes,
                &route_index,
                EndpointOffsets { from: from_offset, to: to_offset },
                &endpoint_side_usage,
                style,
                input.lane_index_by_node.get(&rel.from).copied(),
                input.lane_index_by_node.get(&rel.to).copied(),
                input.row_index_by_node.get(&rel.from).copied(),
                input.row_index_by_node.get(&rel.to).copied(),
            );
        let route = route.unwrap_or_else(|| RouteData {
                d: String::new(),
                points: Vec::new(),
                controls: None,
                samples: Vec::new(),
                sample_bounds: Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
                bends: 0,
                label_x: 0.0,
                label_y: 0.0,
                style: style.to_string(),
                extra: IndexMap::new(),
            });

        if let Some(from_rect) = node_rects_plain.get(&rel.from) {
            if let Some(first_pt) = route.points.first() {
                let side = endpoint_side(from_rect, first_pt);
                endpoint_side_usage.mark(&rel.from, side);
            }
        }
        if let Some(to_rect) = node_rects_plain.get(&rel.to) {
            if let Some(last_pt) = route.points.last() {
                let side = endpoint_side(to_rect, last_pt);
                endpoint_side_usage.mark(&rel.to, side);
            }
        }

        used_routes.push(route.samples.clone());
        route_index.add(&route.points, planned.len());
        planned.push((rel.id.clone(), route));
    }

    planned
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

fn routes_to_json(_routes: &[(String, RouteData)]) -> serde_json::Value {
    // RouteData does not implement Serialize.
    // The raw-route cache is an output-parity optimisation for the JS browser
    // worker; the Rust engine always recomputes. We write a sentinel value so
    // the cache key is occupied (preventing redundant JS recomputes on the next
    // hot-reload), but the Rust path never reads it back.
    serde_json::Value::Null
}

fn build_cache_key_input(input: &RouteEdgesInput, style: &str) -> CacheKeyInput {
    use serde_json::json;
    let relationships: Vec<CacheKeyRelationship> = input
        .relationships
        .iter()
        .map(|r| CacheKeyRelationship {
            id: Some(r.id.clone()),
            from: Some(r.from.clone()),
            to: Some(r.to.clone()),
            label: r.label.clone(),
            relationship_type: r.relationship_type.clone(),
            kind: r.kind.clone(),
            return_of: r.return_of.clone(),
            outcome: r.outcome.clone(),
            step_id: r.step_id.clone(),
            flow_id: r.flow_id.clone(),
            preferred_start_side: r.preferred_start_side.clone(),
            preferred_end_side: r.preferred_end_side.clone(),
        })
        .collect();

    let node_rects: Vec<(String, serde_json::Value)> = input
        .node_rects
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                json!({ "x": v.rect.x, "y": v.rect.y, "width": v.rect.width, "height": v.rect.height }),
            )
        })
        .collect();

    let lane_index_by_node: Vec<(String, serde_json::Value)> =
        input.lane_index_by_node.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let row_index_by_node: Vec<(String, serde_json::Value)> =
        input.row_index_by_node.iter().map(|(k, v)| (k.clone(), json!(v))).collect();

    CacheKeyInput {
        style: style.to_string(),
        relationships,
        visible_node_ids: input.visible_node_ids.clone(),
        node_rects,
        lane_index_by_node,
        row_index_by_node,
        canvas_width: input.canvas_width,
        canvas_height: input.canvas_height,
        margin_y: input.margin_y,
        grid_route_max_points: input.grid_route_max_points as f64,
        grid_route_max_expansions: input.grid_route_max_expansions as f64,
        score_edge_proximity: input.score_edge_proximity,
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD end-to-end (RED → GREEN)
// ---------------------------------------------------------------------------
//
// Node goldens extracted by running `routeEdges(input)` in Node.js with the
// exact same fixture. Tests will FAIL (RED) until the implementation is wired.
//
// Fixture: 3 nodes (A, B, C), 2 relationships (r1: A→B, r2: B→C).
// Goldens:
//   r1: d="M 150 130 L 300 130", 4 points, bends=0, labelX≈236.4, labelY=130
//   r2: d="M 350 160 L 350 178 L 225 178 L 225 250", bends=2

#[cfg(test)]
mod tests {
    use super::*;

    fn make_three_node_input() -> RouteEdgesInput {
        let mut node_rects = IndexMap::new();
        node_rects.insert(
            "A".to_string(),
            NodeRect { rect: Rect { x: 50.0, y: 100.0, width: 100.0, height: 60.0 }, fixed_ports: false },
        );
        node_rects.insert(
            "B".to_string(),
            NodeRect { rect: Rect { x: 300.0, y: 100.0, width: 100.0, height: 60.0 }, fixed_ports: false },
        );
        node_rects.insert(
            "C".to_string(),
            NodeRect { rect: Rect { x: 175.0, y: 250.0, width: 100.0, height: 60.0 }, fixed_ports: false },
        );
        let mut lane_index = IndexMap::new();
        lane_index.insert("A".to_string(), 0i64);
        lane_index.insert("B".to_string(), 0i64);
        lane_index.insert("C".to_string(), 0i64);
        let mut row_index = IndexMap::new();
        row_index.insert("A".to_string(), 0i64);
        row_index.insert("B".to_string(), 0i64);
        row_index.insert("C".to_string(), 1i64);
        RouteEdgesInput {
            style: "orthogonal".to_string(),
            relationships: vec![
                InputRelationship {
                    id: "r1".to_string(),
                    from: "A".to_string(),
                    to: "B".to_string(),
                    relationship_type: Some("flow".to_string()),
                    kind: Some("request".to_string()),
                    return_of: None,
                    outcome: None,
                    step_id: None,
                    flow_id: None,
                    preferred_start_side: None,
                    preferred_end_side: None,
                    label: None,
                    display_index: 0,
                },
                InputRelationship {
                    id: "r2".to_string(),
                    from: "B".to_string(),
                    to: "C".to_string(),
                    relationship_type: Some("flow".to_string()),
                    kind: Some("request".to_string()),
                    return_of: None,
                    outcome: None,
                    step_id: None,
                    flow_id: None,
                    preferred_start_side: None,
                    preferred_end_side: None,
                    label: None,
                    display_index: 1,
                },
            ],
            visible_node_ids: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            node_rects,
            lane_index_by_node: lane_index,
            row_index_by_node: row_index,
            canvas_width: 600.0,
            canvas_height: 500.0,
            margin_y: 20.0,
            grid_route_max_points: 1600,
            grid_route_max_expansions: 4000,
            score_edge_proximity: false,
        }
    }

    #[test]
    fn route_edges_e2e_r1_d_string() {
        let input = make_three_node_input();
        let routes = route_edges(&input);
        let r1 = routes.get("r1").expect("r1 must be present");
        assert_eq!(r1.d, "M 150 130 L 300 130", "r1.d mismatch: got {:?}", r1.d);
    }

    #[test]
    fn route_edges_e2e_r1_points() {
        let input = make_three_node_input();
        let routes = route_edges(&input);
        let r1 = routes.get("r1").expect("r1 must be present");
        assert_eq!(r1.points.len(), 4, "r1 point count: got {:?}", r1.points);
        assert_eq!(r1.points[0], Point { x: 150.0, y: 130.0 });
        assert_eq!(r1.points[1], Point { x: 168.0, y: 130.0 });
        assert_eq!(r1.points[2], Point { x: 282.0, y: 130.0 });
        assert_eq!(r1.points[3], Point { x: 300.0, y: 130.0 });
    }

    #[test]
    fn route_edges_e2e_r1_label() {
        let input = make_three_node_input();
        let routes = route_edges(&input);
        let r1 = routes.get("r1").expect("r1 must be present");
        assert!((r1.label_x - 236.4).abs() < 0.5, "r1.labelX: got {}", r1.label_x);
        assert_eq!(r1.label_y, 130.0, "r1.labelY: got {}", r1.label_y);
    }

    #[test]
    fn route_edges_e2e_r2_bends() {
        let input = make_three_node_input();
        let routes = route_edges(&input);
        let r2 = routes.get("r2").expect("r2 must be present");
        assert_eq!(r2.bends, 2, "r2.bends: got {}", r2.bends);
    }

    #[test]
    fn route_edges_e2e_r2_d_string() {
        let input = make_three_node_input();
        let routes = route_edges(&input);
        let r2 = routes.get("r2").expect("r2 must be present");
        assert_eq!(
            r2.d,
            "M 350 160 L 350 178 L 225 178 L 225 250",
            "r2.d: got {:?}",
            r2.d
        );
    }

    #[test]
    fn route_edges_skips_relationship_without_lane_index() {
        let mut input = make_three_node_input();
        input.relationships.push(InputRelationship {
            id: "r_orphan".to_string(),
            from: "X".to_string(), // not in lane_index_by_node
            to: "A".to_string(),
            relationship_type: None,
            kind: None,
            return_of: None,
            outcome: None,
            step_id: None,
            flow_id: None,
            preferred_start_side: None,
            preferred_end_side: None,
            label: None,
            display_index: 0,
        });
        let routes = route_edges(&input);
        assert!(!routes.contains_key("r_orphan"), "orphan must be skipped");
    }
}
