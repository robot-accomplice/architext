//! Faithful port of `viewer/src/routing/routeCandidateBuilders.js`.
//!
//! ## Scope
//!
//! Exports a `RouteCandidateBuilders` struct that implements the
//! `RouteCandidateFactory` trait from `route_strategies`. Each method
//! is a 1:1 translation of the corresponding JS closure.
//!
//! ## Translation decisions
//!
//! ### Pathfinding (gridRoute / Dijkstra)
//! The JS algorithm is pure Dijkstra (no heuristic), using a binary min-heap
//! keyed on cumulative distance. Exact expansion-order semantics:
//!
//! - `distances` array + `visited` byte array + `previous` array, indexed by
//!   integer grid-point index — all initialised to Infinity / -1 / false.
//! - Start node pushed with distance 0; loop pops minimum, skips if
//!   `item.distance != distances[index]` (stale entry guard), then skips if
//!   `visited[current]`, marks visited, expands neighbours.
//! - **Turn penalty**: 18 px added when the incoming direction differs from the
//!   outgoing direction.  "Same direction" means the axis is the same: check
//!   `points[previous].x == points[current].x && points[current].x == points[next].x`
//!   or the y variant.  The penalty is only applied when `previous[current] >= 0`.
//! - Neighbor list built by iterating (ys × xs_pairwise) then (xs × ys_pairwise),
//!   horizontal edges first.
//! - Path reconstruction: walk `previous` chain from `endIndex` backwards,
//!   `unshift` (prepend) each point, yielding start-first order.
//! - `simplifyOrthogonalPoints([startPort.anchor, ...routePoints, endPort.anchor])`.
//!
//! ### Grid construction
//! - `xLines` / `yLines` start as `{round(start.x), round(end.x), minX, maxX}` /
//!   `{round(start.y), round(end.y), minY, maxY}` — JS `Set`, insertion order
//!   preserved but sorted before use.
//! - Blocker padding lines: `clamp(round(rect.x - padding - offset), minX, maxX)`
//!   etc., using `Math.round` → `js_round`.
//! - Point index keyed by `"x,y"` template string using `Math.round` on both.
//!
//! ### `Math.hypot` / `Math.round` / coordinate formatting
//! - `js_hypot` for every Euclidean distance.
//! - `js_round` for grid coordinate rounding.
//! - All `d`-path coordinates go through `path_to_svg` → `js_number_to_string`.
//!
//! ### Sets / ordering
//! - `xLines`/`yLines` are `IndexSet<i64>` (insertion order before sorting —
//!   then sorted with `sort()`; insertion order is irrelevant once sorted, but
//!   uniqueness matters). We use `IndexSet` for the correct Set dedup semantics.
//! - `pointIndex` Map: insertion order matters because `points.length` is
//!   incremented in order — `IndexMap<String, usize>`.
//! - `visited` is a `Vec<u8>` (1 = visited) mirroring JS `Uint8Array`.
//!
//! ### `withReadableLabel` in `straightCandidate`
//! The JS calls `withReadableLabel(withQualityCosts(...))` which duck-patches
//! `labelX`/`labelY` on the candidate. We inline the same logic on
//! `RouteCandidate` fields directly — avoids an intermediate type conversion
//! and produces identical output.
//!
//! ### Deferred collaborators
//! `routeQualityFromSamples` (collision/crossing/overlap scoring) is part of
//! the Tier 4 integration layer and is injected via the `RouteQualityFn` trait.
//! The concrete type passed in the production plan() call will fill these fields;
//! for unit tests a no-op implementation is used.

use std::cell::RefCell;

use indexmap::{IndexMap, IndexSet};

use crate::js_compat::{js_hypot, js_number_to_string, js_round};
use crate::model::{Point, Rect};
use crate::priority_queue::{HasDistance, MinHeap};
use crate::route_constants::{rect_center, CANVAS_INSET, ROUTE_COST_WEIGHTS};
use crate::route_corridors::CORRIDOR_PADDING;
use crate::route_geometry::{
    bend_count, clamp, line_samples, point_at_distance, route_length, sample_cubic, sample_line,
    segment_intersects_rect, shallow_jog_count, unit_vector,
};
use crate::route_ports::side_vector;
use crate::route_rendering::{path_to_svg, simplify_orthogonal_points};
use crate::route_scoring::{with_quality_costs, QualityCosts, RouteCandidate};
use crate::route_strategies::{CandidateRelationship, RouteCandidateFactory};

// ---------------------------------------------------------------------------
// Default limits (module-level consts matching JS)
// ---------------------------------------------------------------------------

const DEFAULT_GRID_ROUTE_MAX_POINTS: usize = 1600;
const DEFAULT_GRID_ROUTE_MAX_EXPANSIONS: usize = 4000;

// ---------------------------------------------------------------------------
// Stats (mirrors the JS `stats` object fields touched by the builders)
// ---------------------------------------------------------------------------

/// Subset of plan stats fields that the candidate builders mutate.
#[derive(Debug, Clone, Default)]
pub struct BuilderStats {
    pub grid_route_calls: u64,
    pub grid_route_budget_bailouts: u64,
}

// ---------------------------------------------------------------------------
// RouteQualityFn trait
//
// Injected dependency for `routeQualityFromSamples`. In the plan() integration
// layer the real implementation computes collision/crossing/overlap scores;
// in unit tests a zero-returning stub is used.
// ---------------------------------------------------------------------------

/// Port of the JS `routeQualityFromSamples(samples, label, fromId, toId, usedRoutes, relationship)`
/// callback provided by the orchestrator.
pub trait RouteQualityFn {
    fn call(
        &self,
        samples: &[Point],
        label: &Point,
        from_id: &str,
        to_id: &str,
        used_routes: &[Vec<Point>],
        relationship: &CandidateRelationship,
    ) -> QualityCosts;
}

/// No-op implementation for unit tests (returns all-zero costs).
pub struct NoopQuality;

impl RouteQualityFn for NoopQuality {
    fn call(
        &self,
        _samples: &[Point],
        _label: &Point,
        _from_id: &str,
        _to_id: &str,
        _used_routes: &[Vec<Point>],
        _relationship: &CandidateRelationship,
    ) -> QualityCosts {
        QualityCosts::default()
    }
}

// ---------------------------------------------------------------------------
// monotonicBacktrackCost (private helper, mirrors JS closure)
// ---------------------------------------------------------------------------

/// Port of JS `monotonicBacktrackCost(points, fromRect, toRect)`.
///
/// Measures how much the simplified polyline backtracks against the
/// centre-to-centre direction. Each backtracking unit contributes
/// `|distance| * ROUTE_COST_WEIGHTS.monotonicBacktrack`.
fn monotonic_backtrack_cost(points: &[Point], from_rect: &Rect, to_rect: &Rect) -> f64 {
    let from_center = rect_center(from_rect);
    let to_center = rect_center(to_rect);
    let x_direction = f64::signum(to_center.x - from_center.x);
    let y_direction = f64::signum(to_center.y - from_center.y);
    let mut cost = 0.0_f64;
    for index in 0..points.len().saturating_sub(1) {
        let start = &points[index];
        let end = &points[index + 1];
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        if x_direction != 0.0 && f64::signum(dx) == -x_direction {
            cost += f64::abs(dx) * ROUTE_COST_WEIGHTS.monotonic_backtrack;
        }
        if y_direction != 0.0 && f64::signum(dy) == -y_direction {
            cost += f64::abs(dy) * ROUTE_COST_WEIGHTS.monotonic_backtrack;
        }
    }
    cost
}

// ---------------------------------------------------------------------------
// PortResult (mirrors the JS { port, anchor } shape)
// ---------------------------------------------------------------------------

/// The two points that describe one endpoint of a candidate.
#[derive(Debug, Clone)]
pub struct PortResult {
    /// The surface mount point (on the node boundary).
    pub port: Point,
    /// The stub anchor (protruding from the port into the canvas).
    pub anchor: Point,
}

// ---------------------------------------------------------------------------
// RouteCandidateBuilders
// ---------------------------------------------------------------------------

/// The candidate factory.
///
/// Created once per plan call via `RouteCandidateBuilders::new(context)`.
/// Methods correspond to the JS closures returned by `createRouteCandidateFactory`.
pub struct RouteCandidateBuilders<Q: RouteQualityFn> {
    /// Blocker rects callback: given (fromId, toId), returns the rects that
    /// obstruct routing between them (excludes the endpoint nodes themselves).
    #[allow(clippy::type_complexity)]
    pub blocker_rects: Box<dyn Fn(&str, &str) -> Vec<Rect>>,
    pub canvas_height: f64,
    pub canvas_width: f64,
    /// Bounding box of all nodes (used to compute perimeterBounds).
    pub node_bounds: Option<NodeBounds>,
    pub grid_route_max_expansions: usize,
    pub grid_route_max_points: usize,
    /// Rect lookup by node id.
    pub rect_for: Box<dyn Fn(&str) -> Rect>,
    /// Quality scoring callback.
    pub route_quality: Q,
    /// Mutable stats (optionally collected). Wrapped in RefCell so the
    /// RouteCandidateFactory trait methods can mutate stats through &self.
    pub stats: RefCell<Option<BuilderStats>>,
    /// Progress heartbeat fired each time gridRoute is called.
    pub progress_tick: Option<Box<dyn Fn()>>,
    /// Precomputed perimeter bounds (set in `new`).
    perimeter_bounds: PerimeterBounds,
}

/// Bounding box of all nodes: `{ minX, minY, maxX, maxY }`.
#[derive(Debug, Clone, Copy)]
pub struct NodeBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// The perimeter gutter box, computed once in `new`.
#[derive(Debug, Clone, Copy)]
struct PerimeterBounds {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

const PERIMETER_GUTTER_MARGIN: f64 = 24.0;

impl PerimeterBounds {
    fn compute(node_bounds: Option<NodeBounds>, canvas_width: f64, canvas_height: f64) -> Self {
        let left = match node_bounds {
            Some(nb) => f64::max(CANVAS_INSET.left, nb.min_x - PERIMETER_GUTTER_MARGIN),
            None => CANVAS_INSET.left,
        };
        let right = match node_bounds {
            Some(nb) => f64::min(canvas_width - CANVAS_INSET.right, nb.max_x + PERIMETER_GUTTER_MARGIN),
            None => canvas_width - CANVAS_INSET.right,
        };
        let top = match node_bounds {
            Some(nb) => f64::max(CANVAS_INSET.top, nb.min_y - PERIMETER_GUTTER_MARGIN),
            None => CANVAS_INSET.top,
        };
        let bottom = match node_bounds {
            Some(nb) => f64::min(canvas_height - CANVAS_INSET.bottom, nb.max_y + PERIMETER_GUTTER_MARGIN),
            None => canvas_height - CANVAS_INSET.bottom,
        };
        PerimeterBounds { left, right, top, bottom }
    }
}

impl<Q: RouteQualityFn> RouteCandidateBuilders<Q> {
    /// Construct a new factory, computing `perimeterBounds` eagerly.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        blocker_rects: impl Fn(&str, &str) -> Vec<Rect> + 'static,
        canvas_width: f64,
        canvas_height: f64,
        node_bounds: Option<NodeBounds>,
        grid_route_max_expansions: Option<usize>,
        grid_route_max_points: Option<usize>,
        rect_for: impl Fn(&str) -> Rect + 'static,
        route_quality: Q,
        stats: Option<BuilderStats>,
        progress_tick: Option<Box<dyn Fn()>>,
    ) -> Self {
        let perimeter_bounds = PerimeterBounds::compute(node_bounds, canvas_width, canvas_height);
        RouteCandidateBuilders {
            blocker_rects: Box::new(blocker_rects),
            canvas_height,
            canvas_width,
            node_bounds,
            grid_route_max_expansions: grid_route_max_expansions
                .unwrap_or(DEFAULT_GRID_ROUTE_MAX_EXPANSIONS),
            grid_route_max_points: grid_route_max_points.unwrap_or(DEFAULT_GRID_ROUTE_MAX_POINTS),
            rect_for: Box::new(rect_for),
            route_quality,
            stats: RefCell::new(stats),
            progress_tick,
            perimeter_bounds,
        }
    }

    // -----------------------------------------------------------------------
    // gridRoute (private — called from RouteCandidateFactory impl)
    // -----------------------------------------------------------------------

    /// Port of JS `gridRoute(relationship, fromId, toId, startSide, endSide,
    ///   routeOffset, usedRoutes, startPort, endPort)`.
    ///
    /// Pure Dijkstra on an orthogonal grid constructed from blocker boundaries.
    /// Returns `None` on budget bailout or no path.
    #[allow(clippy::too_many_arguments)]
    fn grid_route_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        route_offset: f64,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
    ) -> Option<RouteCandidate> {
        if let Some(ref mut s) = *self.stats.borrow_mut() {
            s.grid_route_calls += 1;
        }
        if let Some(ref tick) = self.progress_tick {
            tick();
        }

        let start = &start_port.port;
        let end = &end_port.port;
        let from_rect = (self.rect_for)(from_id);
        let to_rect = (self.rect_for)(to_id);
        let blockers = (self.blocker_rects)(from_id, to_id);
        let padding = CORRIDOR_PADDING;
        let min_x = CANVAS_INSET.left;
        let max_x = self.canvas_width - CANVAS_INSET.right;
        let min_y = CANVAS_INSET.top;
        let max_y = self.canvas_height - CANVAS_INSET.bottom;

        // Build coordinate sets. JS Set insertion order is preserved but then
        // sorted, so uniqueness is what matters; we use IndexSet for clarity.
        let mut x_lines: IndexSet<i64> = IndexSet::new();
        let mut y_lines: IndexSet<i64> = IndexSet::new();

        let add_x = |set: &mut IndexSet<i64>, value: f64| {
            let clamped = f64::min(max_x, f64::max(min_x, js_round(value)));
            set.insert(clamped as i64);
        };
        let add_y = |set: &mut IndexSet<i64>, value: f64| {
            let clamped = f64::min(max_y, f64::max(min_y, js_round(value)));
            set.insert(clamped as i64);
        };

        x_lines.insert(js_round(start.x) as i64);
        x_lines.insert(js_round(end.x) as i64);
        x_lines.insert(min_x as i64);
        x_lines.insert(max_x as i64);

        y_lines.insert(js_round(start.y) as i64);
        y_lines.insert(js_round(end.y) as i64);
        y_lines.insert(min_y as i64);
        y_lines.insert(max_y as i64);

        for rect in &blockers {
            add_x(&mut x_lines, rect.x - padding - route_offset);
            add_x(&mut x_lines, rect.x + rect.width + padding + route_offset);
            add_y(&mut y_lines, rect.y - padding - route_offset);
            add_y(&mut y_lines, rect.y + rect.height + padding + route_offset);
        }

        // Sort ascending (JS: [...xLines].sort((a,b)=>a-b))
        let mut xs: Vec<i64> = x_lines.into_iter().collect();
        xs.sort_unstable();
        let mut ys: Vec<i64> = y_lines.into_iter().collect();
        ys.sort_unstable();

        // Build point list and index map.
        // JS iterates xs outer, ys inner: for x of xs { for y of ys { ... } }
        let mut points: Vec<Point> = Vec::new();
        let mut point_index: IndexMap<String, usize> = IndexMap::new();
        for &x in &xs {
            for &y in &ys {
                let key = format!("{},{}", x, y);
                point_index.insert(key, points.len());
                points.push(Point { x: x as f64, y: y as f64 });
            }
        }

        if points.len() > self.grid_route_max_points {
            if let Some(ref mut s) = *self.stats.borrow_mut() {
                s.grid_route_budget_bailouts += 1;
            }
            return None;
        }

        // Locate start and end in the index.
        let point_key = |p: &Point| {
            format!(
                "{},{}",
                js_round(p.x) as i64,
                js_round(p.y) as i64
            )
        };
        let start_index = *point_index.get(&point_key(start))?;
        let end_index = *point_index.get(&point_key(end))?;

        // Build adjacency lists.
        // JS order: horizontal edges (ys outer, xs pair inner), then vertical.
        let mut neighbors: Vec<Vec<(usize, f64)>> = vec![Vec::new(); points.len()];

        // Precompute blocker lookup by y-row and x-column.
        // horizontalBlockersByY: Map(y => blockers where y > rect.y - padding && y < rect.y + rect.height + padding)
        let horizontal_blockers_by_y: std::collections::HashMap<i64, Vec<&Rect>> = {
            let mut map: std::collections::HashMap<i64, Vec<&Rect>> = std::collections::HashMap::new();
            for &y in &ys {
                let yf = y as f64;
                let filtered: Vec<&Rect> = blockers
                    .iter()
                    .filter(|rect| yf > rect.y - padding && yf < rect.y + rect.height + padding)
                    .collect();
                map.insert(y, filtered);
            }
            map
        };
        let vertical_blockers_by_x: std::collections::HashMap<i64, Vec<&Rect>> = {
            let mut map: std::collections::HashMap<i64, Vec<&Rect>> = std::collections::HashMap::new();
            for &x in &xs {
                let xf = x as f64;
                let filtered: Vec<&Rect> = blockers
                    .iter()
                    .filter(|rect| xf > rect.x - padding && xf < rect.x + rect.width + padding)
                    .collect();
                map.insert(x, filtered);
            }
            map
        };

        let horizontal_clear = |y: i64, left: i64, right: i64| -> bool {
            let lf = left as f64;
            let rf = right as f64;
            let (min_xf, max_xf) = (f64::min(lf, rf), f64::max(lf, rf));
            horizontal_blockers_by_y
                .get(&y)
                .map(|rects| {
                    rects.iter().all(|rect| {
                        max_xf <= rect.x - padding || min_xf >= rect.x + rect.width + padding
                    })
                })
                .unwrap_or(true)
        };
        let vertical_clear = |x: i64, top: i64, bottom: i64| -> bool {
            let tf = top as f64;
            let bf = bottom as f64;
            let (min_yf, max_yf) = (f64::min(tf, bf), f64::max(tf, bf));
            vertical_blockers_by_x
                .get(&x)
                .map(|rects| {
                    rects.iter().all(|rect| {
                        max_yf <= rect.y - padding || min_yf >= rect.y + rect.height + padding
                    })
                })
                .unwrap_or(true)
        };

        // Horizontal edges (JS: for y of ys, for index 0..xs.len-1)
        for &y in &ys {
            for index in 0..xs.len().saturating_sub(1) {
                let xa = xs[index];
                let xb = xs[index + 1];
                let key_a = format!("{},{}", xa, y);
                let key_b = format!("{},{}", xb, y);
                let a = *point_index.get(&key_a).unwrap();
                let b = *point_index.get(&key_b).unwrap();
                if horizontal_clear(y, xa, xb) {
                    let distance = f64::abs((xb - xa) as f64);
                    neighbors[a].push((b, distance));
                    neighbors[b].push((a, distance));
                }
            }
        }
        // Vertical edges (JS: for x of xs, for index 0..ys.len-1)
        for &x in &xs {
            for index in 0..ys.len().saturating_sub(1) {
                let ya = ys[index];
                let yb = ys[index + 1];
                let key_a = format!("{},{}", x, ya);
                let key_b = format!("{},{}", x, yb);
                let a = *point_index.get(&key_a).unwrap();
                let b = *point_index.get(&key_b).unwrap();
                if vertical_clear(x, ya, yb) {
                    let distance = f64::abs((yb - ya) as f64);
                    neighbors[a].push((b, distance));
                    neighbors[b].push((a, distance));
                }
            }
        }

        // Dijkstra
        let mut distances: Vec<f64> = vec![f64::INFINITY; points.len()];
        let mut previous: Vec<i64> = vec![-1_i64; points.len()];
        let mut visited: Vec<u8> = vec![0u8; points.len()];
        let mut queue: MinHeap<GridItem> = MinHeap::new();
        distances[start_index] = 0.0;
        queue.push(GridItem { index: start_index, distance: 0.0 });
        let mut expansions: usize = 0;

        while !queue.is_empty() {
            let next_item = match queue.pop() {
                Some(item) => item,
                None => break,
            };
            // JS: if (!nextItem || nextItem.distance !== distances[nextItem.index]) continue;
            if next_item.distance != distances[next_item.index] {
                continue;
            }
            let current = next_item.index;
            if current == end_index {
                break;
            }
            if visited[current] != 0 {
                continue;
            }
            visited[current] = 1;
            expansions += 1;
            if expansions > self.grid_route_max_expansions {
                if let Some(ref mut s) = *self.stats.borrow_mut() {
                    s.grid_route_budget_bailouts += 1;
                }
                return None;
            }

            for &(next, dist) in &neighbors[current] {
                if visited[next] != 0 {
                    continue;
                }
                // Turn penalty: 18 when direction changes.
                // JS: (points[previous[current]].x !== points[current].x && points[current].x !== points[next].x)
                //  || (points[previous[current]].y !== points[current].y && points[current].y !== points[next].y)
                let turn_penalty = if previous[current] >= 0 {
                    let prev_idx = previous[current] as usize;
                    let prev_pt = &points[prev_idx];
                    let curr_pt = &points[current];
                    let next_pt = &points[next];
                    let x_turn = prev_pt.x != curr_pt.x && curr_pt.x != next_pt.x;
                    let y_turn = prev_pt.y != curr_pt.y && curr_pt.y != next_pt.y;
                    if x_turn || y_turn { 18.0 } else { 0.0 }
                } else {
                    0.0
                };
                let next_distance = distances[current] + dist + turn_penalty;
                if next_distance < distances[next] {
                    distances[next] = next_distance;
                    previous[next] = current as i64;
                    queue.push(GridItem { index: next, distance: next_distance });
                }
            }
        }

        if !distances[end_index].is_finite() {
            return None;
        }

        // Reconstruct path (JS: unshift = prepend)
        let mut route_points: Vec<Point> = Vec::new();
        let mut cursor = end_index as i64;
        while cursor != -1 {
            route_points.insert(0, points[cursor as usize].clone());
            cursor = previous[cursor as usize];
        }

        // simplifyOrthogonalPoints([startPort.anchor, ...routePoints, endPort.anchor])
        let mut full_points = vec![start_port.anchor.clone()];
        full_points.extend(route_points);
        full_points.push(end_port.anchor.clone());
        let simplified = simplify_orthogonal_points(&full_points);

        let samples = line_samples(&simplified);
        let label = samples.get(samples.len() / 2).cloned().unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });
        let backtrack_cost = monotonic_backtrack_cost(&simplified, &from_rect, &to_rect);

        let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);

        let bends = bend_count(&simplified) as i64;
        Some(with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&simplified),
                label_x: label.x,
                label_y: label.y,
                bends,
                samples,
                points: simplified.clone(),
                start_side: Some(start_side.to_string()),
                end_side: Some(end_side.to_string()),
                ..RouteCandidate::default()
            },
            QualityCosts {
                point_count_cost: simplified.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
                bend_cost: bend_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.bend,
                dogleg_cost: shallow_jog_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.dogleg,
                monotonic_backtrack_cost: backtrack_cost,
                ..quality
            },
        ))
    }

    // -----------------------------------------------------------------------
    // splineCandidate (private)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn spline_candidate_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
        pair_index: usize,
        curvature_offset: f64,
    ) -> Option<RouteCandidate> {
        let start = &start_port.anchor;
        let end = &end_port.anchor;
        let center_distance = js_hypot(end.x - start.x, end.y - start.y);
        let control_distance = clamp(
            center_distance * 0.32 + pair_index as f64 * 12.0,
            64.0,
            190.0,
        );
        let chord = unit_vector(start, end);
        let normal = Point { x: -chord.y, y: chord.x };
        let start_vector = side_vector(start_side);
        let end_vector = side_vector(end_side);
        let direction = Point { x: end.x - start.x, y: end.y - start.y };
        let start_direction = start_vector.x * direction.x + start_vector.y * direction.y;
        let end_direction = end_vector.x * direction.x + end_vector.y * direction.y;
        let side_direction_cost = (if start_direction < 0.0 {
            f64::abs(start_direction) * ROUTE_COST_WEIGHTS.side_direction
        } else {
            0.0
        }) + (if end_direction > 0.0 {
            f64::abs(end_direction) * ROUTE_COST_WEIGHTS.side_direction
        } else {
            0.0
        });

        let clamp_x = |v: f64| clamp(v, CANVAS_INSET.left, self.canvas_width - CANVAS_INSET.right);
        let clamp_y = |v: f64| clamp(v, CANVAS_INSET.top, self.canvas_height - CANVAS_INSET.bottom);

        let control_a = Point {
            x: clamp_x(start.x + chord.x * control_distance + normal.x * curvature_offset),
            y: clamp_y(start.y + chord.y * control_distance + normal.y * curvature_offset),
        };
        let control_b = Point {
            x: clamp_x(end.x - chord.x * control_distance + normal.x * curvature_offset),
            y: clamp_y(end.y - chord.y * control_distance + normal.y * curvature_offset),
        };

        // samples = [start, ...sampleCubic(start, controlA, controlB, end, 32)]
        let cubic_samples = sample_cubic(start, &control_a, &control_b, end, 32);
        let mut samples = vec![start.clone()];
        samples.extend(cubic_samples);

        let label = point_at_distance(&samples, route_length(&samples) / 2.0)
            .unwrap_or_else(|| Point {
                x: (start.x + end.x) / 2.0,
                y: (start.y + end.y) / 2.0,
            });

        // JS d string: `M ${start.x} ${start.y} C ${controlA.x} ${controlA.y} ${controlB.x} ${controlB.y} ${end.x} ${end.y}`
        let d = format!(
            "M {} {} C {} {} {} {} {} {}",
            js_number_to_string(start.x),
            js_number_to_string(start.y),
            js_number_to_string(control_a.x),
            js_number_to_string(control_a.y),
            js_number_to_string(control_b.x),
            js_number_to_string(control_b.y),
            js_number_to_string(end.x),
            js_number_to_string(end.y),
        );

        let length_cost = route_length(&samples);
        let spline_straightness_cost = if f64::abs(curvature_offset) < 1.0 {
            ROUTE_COST_WEIGHTS.spline_flat_penalty
        } else {
            0.0
        };

        let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);

        Some(with_quality_costs(
            RouteCandidate {
                d,
                label_x: label.x,
                label_y: label.y,
                bends: 0,
                samples,
                points: vec![start.clone(), end.clone()],
                start_side: Some(start_side.to_string()),
                end_side: Some(end_side.to_string()),
                style: "spline".to_string(),
                ..RouteCandidate::default()
            },
            QualityCosts {
                length_cost,
                point_count_cost: 2.0 * ROUTE_COST_WEIGHTS.point_count,
                directness_reward: ROUTE_COST_WEIGHTS.spline_reward,
                spline_side_direction_cost: side_direction_cost,
                spline_straightness_cost,
                ..quality
            },
        ))
    }

    // -----------------------------------------------------------------------
    // perimeterRoute (private)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn perimeter_route_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        side: &str,
        route_offset: f64,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
    ) -> Option<RouteCandidate> {
        let start = &start_port.port;
        let end = &end_port.port;
        let gutter = match side {
            "left" => self.perimeter_bounds.left + route_offset,
            "right" => self.perimeter_bounds.right - route_offset,
            "top" => self.perimeter_bounds.top + route_offset,
            _ => self.perimeter_bounds.bottom - route_offset,
        };
        let raw_points: Vec<Point> = if side == "left" || side == "right" {
            vec![
                start_port.anchor.clone(),
                start.clone(),
                Point { x: gutter, y: start.y },
                Point { x: gutter, y: end.y },
                end.clone(),
                end_port.anchor.clone(),
            ]
        } else {
            vec![
                start_port.anchor.clone(),
                start.clone(),
                Point { x: start.x, y: gutter },
                Point { x: end.x, y: gutter },
                end.clone(),
                end_port.anchor.clone(),
            ]
        };
        let simplified = simplify_orthogonal_points(&raw_points);
        let samples = line_samples(&simplified);
        let label = samples.get(samples.len() / 2).cloned().unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });

        let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);
        let bends = bend_count(&simplified) as i64;
        Some(with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&simplified),
                label_x: label.x,
                label_y: label.y,
                bends,
                samples,
                points: simplified.clone(),
                start_side: Some(side.to_string()),
                end_side: Some(side.to_string()),
                ..RouteCandidate::default()
            },
            QualityCosts {
                perimeter_fallback_cost: ROUTE_COST_WEIGHTS.perimeter_fallback,
                perimeter_length_cost: route_length(&simplified) * ROUTE_COST_WEIGHTS.perimeter_length,
                point_count_cost: simplified.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
                bend_cost: bend_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.bend,
                dogleg_cost: shallow_jog_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.dogleg,
                ..quality
            },
        ))
    }

    // -----------------------------------------------------------------------
    // cornerPerimeterRoutes (private)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn corner_perimeter_routes_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        route_offset: f64,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
    ) -> Vec<RouteCandidate> {
        let pb = &self.perimeter_bounds;
        let boundaries = [
            Point { x: pb.left + route_offset,  y: pb.top + route_offset },
            Point { x: pb.right - route_offset, y: pb.top + route_offset },
            Point { x: pb.left + route_offset,  y: pb.bottom - route_offset },
            Point { x: pb.right - route_offset, y: pb.bottom - route_offset },
        ];

        let start = &start_port.port;
        let end = &end_port.port;

        // JS: boundaries.flatMap(boundary => [path1, path2]).map(...)
        // path1: horizontal-first approach
        // path2: vertical-first approach
        let mut result = Vec::new();

        for boundary in &boundaries {
            let path1 = vec![
                start_port.anchor.clone(),
                start.clone(),
                Point { x: boundary.x, y: start.y },
                boundary.clone(),
                Point { x: boundary.x, y: end.y },
                end.clone(),
                end_port.anchor.clone(),
            ];
            let path2 = vec![
                start_port.anchor.clone(),
                start.clone(),
                Point { x: start.x, y: boundary.y },
                boundary.clone(),
                Point { x: end.x, y: boundary.y },
                end.clone(),
                end_port.anchor.clone(),
            ];

            for raw_points in [path1, path2] {
                let simplified = simplify_orthogonal_points(&raw_points);
                let samples = line_samples(&simplified);
                // JS uses simplified[0] and simplified[simplified.length-1] for label fallback
                let first = simplified.first().cloned().unwrap_or(start.clone());
                let last = simplified.last().cloned().unwrap_or(end.clone());
                let label = samples.get(samples.len() / 2).cloned().unwrap_or_else(|| Point {
                    x: (first.x + last.x) / 2.0,
                    y: (first.y + last.y) / 2.0,
                });
                let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);
                let bends = bend_count(&simplified) as i64;
                result.push(with_quality_costs(
                    RouteCandidate {
                        d: path_to_svg(&simplified),
                        label_x: label.x,
                        label_y: label.y,
                        bends,
                        samples,
                        points: simplified.clone(),
                        ..RouteCandidate::default()
                    },
                    QualityCosts {
                        perimeter_fallback_cost: ROUTE_COST_WEIGHTS.corner_perimeter_fallback,
                        perimeter_length_cost: route_length(&simplified)
                            * ROUTE_COST_WEIGHTS.corner_perimeter_length,
                        point_count_cost: simplified.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
                        bend_cost: bend_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.bend,
                        dogleg_cost: shallow_jog_count(&simplified) as f64
                            * ROUTE_COST_WEIGHTS.dogleg,
                        ..quality
                    },
                ));
            }
        }

        result
    }

    // -----------------------------------------------------------------------
    // directPortCandidate (private)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn direct_port_candidate_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
    ) -> Option<RouteCandidate> {
        let start_vector = side_vector(start_side);
        let end_vector = side_vector(end_side);
        let horizontal = start_port.port.y == end_port.port.y
            && start_vector.y == 0.0
            && end_vector.y == 0.0;
        let vertical = start_port.port.x == end_port.port.x
            && start_vector.x == 0.0
            && end_vector.x == 0.0;
        if !horizontal && !vertical {
            return None;
        }

        let points = simplify_orthogonal_points(&[
            start_port.anchor.clone(),
            start_port.port.clone(),
            end_port.port.clone(),
            end_port.anchor.clone(),
        ]);
        let blockers = (self.blocker_rects)(from_id, to_id);
        // JS: blockers.every(rect => points.slice(0,-1).every((point,index) => !segmentIntersectsRect(point, points[index+1], rect, 0)))
        let all_clear = blockers.iter().all(|rect| {
            points
                .iter()
                .zip(points.iter().skip(1))
                .all(|(p, next)| !segment_intersects_rect(p, next, rect, 0.0))
        });
        if !all_clear {
            return None;
        }

        let samples = line_samples(&points);
        let label = samples.get(samples.len() / 2).cloned().unwrap_or_else(|| Point {
            x: (start_port.anchor.x + end_port.anchor.x) / 2.0,
            y: (start_port.anchor.y + end_port.anchor.y) / 2.0,
        });

        let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);
        let bends = bend_count(&points) as i64;
        Some(with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&points),
                label_x: label.x,
                label_y: label.y,
                bends,
                samples,
                points: points.clone(),
                start_side: Some(start_side.to_string()),
                end_side: Some(end_side.to_string()),
                ..RouteCandidate::default()
            },
            QualityCosts {
                directness_reward: ROUTE_COST_WEIGHTS.direct_port_reward,
                dogleg_cost: shallow_jog_count(&points) as f64 * ROUTE_COST_WEIGHTS.dogleg,
                ..quality
            },
        ))
    }

    // -----------------------------------------------------------------------
    // straightCandidate (private)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn straight_candidate_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
    ) -> Option<RouteCandidate> {
        let points = vec![start_port.anchor.clone(), end_port.anchor.clone()];
        let samples = sample_line(&points[0], &points[1], 18);
        let label = samples.get(samples.len() / 2).cloned().unwrap_or_else(|| Point {
            x: (points[0].x + points[1].x) / 2.0,
            y: (points[0].y + points[1].y) / 2.0,
        });

        let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);
        let length_cost = route_length(&samples);

        let mut candidate = with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&points),
                label_x: label.x,
                label_y: label.y,
                bends: 0,
                samples: samples.clone(),
                points: points.clone(),
                start_side: Some(start_side.to_string()),
                end_side: Some(end_side.to_string()),
                style: "straight".to_string(),
                ..RouteCandidate::default()
            },
            QualityCosts {
                length_cost,
                point_count_cost: ROUTE_COST_WEIGHTS.point_count,
                directness_reward: ROUTE_COST_WEIGHTS.straight_reward,
                ..quality
            },
        );

        // Apply withReadableLabel semantics directly on RouteCandidate.
        // JS: withReadableLabel(withQualityCosts(...))
        // withReadableLabel shifts labelX/labelY on short routes.
        let total_length = route_length(&samples);
        if total_length < 70.0 {
            if let Some(first) = points.first() {
                let is_vertical = points.iter().all(|p| p.x == first.x);
                let is_horizontal = points.iter().all(|p| p.y == first.y);
                if is_vertical {
                    candidate.label_x += 28.0;
                } else if is_horizontal {
                    candidate.label_y -= 22.0;
                }
            }
        }

        Some(candidate)
    }

    // -----------------------------------------------------------------------
    // corridorCandidate (private)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn corridor_candidate_impl(
        &self,
        relationship: &CandidateRelationship,
        from_id: &str,
        to_id: &str,
        start_side: &str,
        end_side: &str,
        used_routes: &[Vec<Point>],
        start_port: &PortResult,
        end_port: &PortResult,
        corridor_axis: &str,
        corridor_value: f64,
    ) -> Option<RouteCandidate> {
        let start = &start_port.port;
        let end = &end_port.port;
        let raw_points: Vec<Point> = if corridor_axis == "x" {
            vec![
                start_port.anchor.clone(),
                start.clone(),
                Point { x: corridor_value, y: start.y },
                Point { x: corridor_value, y: end.y },
                end.clone(),
                end_port.anchor.clone(),
            ]
        } else {
            vec![
                start_port.anchor.clone(),
                start.clone(),
                Point { x: start.x, y: corridor_value },
                Point { x: end.x, y: corridor_value },
                end.clone(),
                end_port.anchor.clone(),
            ]
        };
        let simplified = simplify_orthogonal_points(&raw_points);
        let blockers = (self.blocker_rects)(from_id, to_id);
        // JS: blockers.every(rect => simplified.slice(0,-1).every((point,index) => !segmentIntersectsRect(..., CORRIDOR_PADDING)))
        let all_clear = blockers.iter().all(|rect| {
            simplified
                .iter()
                .zip(simplified.iter().skip(1))
                .all(|(p, next)| !segment_intersects_rect(p, next, rect, CORRIDOR_PADDING))
        });
        if !all_clear {
            return None;
        }

        let samples = line_samples(&simplified);
        let label = samples.get(samples.len() / 2).cloned().unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });
        let from_rect = (self.rect_for)(from_id);
        let to_rect = (self.rect_for)(to_id);
        let backtrack_cost = monotonic_backtrack_cost(&simplified, &from_rect, &to_rect);
        let quality = self.route_quality.call(&samples, &label, from_id, to_id, used_routes, relationship);
        let bends = bend_count(&simplified) as i64;
        Some(with_quality_costs(
            RouteCandidate {
                d: path_to_svg(&simplified),
                label_x: label.x,
                label_y: label.y,
                bends,
                samples,
                points: simplified.clone(),
                start_side: Some(start_side.to_string()),
                end_side: Some(end_side.to_string()),
                ..RouteCandidate::default()
            },
            QualityCosts {
                point_count_cost: simplified.len() as f64 * ROUTE_COST_WEIGHTS.point_count,
                bend_cost: bend_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.bend,
                dogleg_cost: shallow_jog_count(&simplified) as f64 * ROUTE_COST_WEIGHTS.dogleg,
                monotonic_backtrack_cost: backtrack_cost,
                ..quality
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// GridItem: heap item for the Dijkstra priority queue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GridItem {
    index: usize,
    distance: f64,
}

impl HasDistance for GridItem {
    fn distance(&self) -> f64 {
        self.distance
    }
}

// ---------------------------------------------------------------------------
// RouteCandidateFactory impl
//
// The trait takes anchors/ports as separate Point arguments (matching the
// route_strategies.rs trait definition). We reconstruct PortResult internally.
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
impl<Q: RouteQualityFn> RouteCandidateFactory for RouteCandidateBuilders<Q> {
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
    ) -> Option<RouteCandidate> {
        self.direct_port_candidate_impl(
            relationship,
            from_id,
            to_id,
            start_side,
            end_side,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
        )
    }

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
    ) -> Option<RouteCandidate> {
        self.corridor_candidate_impl(
            relationship,
            from_id,
            to_id,
            start_side,
            end_side,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
            corridor_axis,
            corridor_value,
        )
    }

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
    ) -> Option<RouteCandidate> {
        self.grid_route_impl(
            relationship,
            from_id,
            to_id,
            start_side,
            end_side,
            route_offset,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
        )
    }

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
    ) -> Option<RouteCandidate> {
        self.perimeter_route_impl(
            relationship,
            from_id,
            to_id,
            side,
            route_offset,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
        )
    }

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
    ) -> Vec<RouteCandidate> {
        self.corner_perimeter_routes_impl(
            relationship,
            from_id,
            to_id,
            route_offset,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
        )
    }

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
    ) -> Option<RouteCandidate> {
        self.spline_candidate_impl(
            relationship,
            from_id,
            to_id,
            start_side,
            end_side,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
            pair_index,
            offset,
        )
    }

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
    ) -> Option<RouteCandidate> {
        self.straight_candidate_impl(
            relationship,
            from_id,
            to_id,
            start_side,
            end_side,
            used_routes,
            &PortResult { port: start_port.clone(), anchor: start_anchor.clone() },
            &PortResult { port: end_port.clone(), anchor: end_anchor.clone() },
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Rect};
    use crate::route_strategies::CandidateRelationship;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }
    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    /// Construct a minimal test factory with two nodes and optional blockers.
    fn make_factory(
        from_rect: Rect,
        to_rect: Rect,
        canvas_width: f64,
        canvas_height: f64,
        node_bounds: Option<NodeBounds>,
        blockers: Vec<Rect>,
    ) -> RouteCandidateBuilders<NoopQuality> {
        let from_clone = from_rect.clone();
        let to_clone = to_rect.clone();
        RouteCandidateBuilders::new(
            move |_from_id, _to_id| blockers.clone(),
            canvas_width,
            canvas_height,
            node_bounds,
            None,
            None,
            move |id: &str| {
                if id == "A" { from_clone.clone() } else { to_clone.clone() }
            },
            NoopQuality,
            None,
            None,
        )
    }

    fn no_rel() -> CandidateRelationship {
        CandidateRelationship::default()
    }

    // -----------------------------------------------------------------------
    // monotonicBacktrackCost
    // -----------------------------------------------------------------------

    #[test]
    fn monotonic_backtrack_zero_forward() {
        // Node: from=(0,0,100,50) to=(200,0,100,50), points going right → cost 0
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        let points = vec![pt(100.0, 25.0), pt(200.0, 25.0)];
        assert_eq!(monotonic_backtrack_cost(&points, &from, &to), 0.0);
    }

    #[test]
    fn monotonic_backtrack_backward_x() {
        // Node: points going left on x (backtrack), dx=100 * 18 = 1800
        let from = rect(0.0, 0.0, 100.0, 50.0);
        let to = rect(200.0, 0.0, 100.0, 50.0);
        // toCenter.x=250 > fromCenter.x=50 → xDirection=+1
        // dx = 100-200 = -100 → sign(-100) = -1 = -xDirection → cost += 100*18 = 1800
        let points = vec![pt(200.0, 25.0), pt(100.0, 25.0)];
        assert_eq!(monotonic_backtrack_cost(&points, &from, &to), 1800.0);
    }

    // -----------------------------------------------------------------------
    // gridRoute — golden from Node
    // -----------------------------------------------------------------------

    #[test]
    fn grid_route_blocker_goes_around() {
        // Node golden:
        // gridRoute with one blocker between nodes; A* routes around it.
        // d: "M 118 25 L 100 25 L 100 30 L 110 30 L 110 60 L 190 60 L 190 30 L 200 30 L 200 25 L 182 25"
        // points: [{118,25},{100,25},{100,30},{110,30},{110,60},{190,60},{190,30},{200,30},{200,25},{182,25}]
        // bends: 8, labelX: 158, labelY: 60

        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let blocker = rect(120.0, 0.0, 60.0, 50.0);
        let f = make_factory(
            from_rect,
            to_rect,
            600.0,
            400.0,
            Some(NodeBounds { min_x: 0.0, min_y: 0.0, max_x: 300.0, max_y: 50.0 }),
            vec![blocker],
        );

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let r = f.grid_route_impl(&no_rel(), "A", "B", "right", "left", 0.0, &[], &start_port, &end_port);
        let r = r.expect("expected Some route");

        assert_eq!(r.d, "M 118 25 L 100 25 L 100 30 L 110 30 L 110 60 L 190 60 L 190 30 L 200 30 L 200 25 L 182 25");
        assert_eq!(r.bends, 8);
        assert_eq!(r.label_x, 158.0);
        assert_eq!(r.label_y, 60.0);
        assert_eq!(r.points.len(), 10);
        assert_eq!(r.points[0], pt(118.0, 25.0));
        assert_eq!(r.points[9], pt(182.0, 25.0));
    }

    #[test]
    fn grid_route_no_blocker_direct_path() {
        // Node golden: no blockers → straight line through the grid
        // d: "M 58 20 L 82 20", points: [{58,20},{82,20}]
        let from_rect = rect(0.0, 0.0, 40.0, 40.0);
        let to_rect = rect(100.0, 0.0, 40.0, 40.0);
        let f = make_factory(from_rect, to_rect, 200.0, 100.0, None, vec![]);

        let start_port = PortResult { port: pt(40.0, 20.0), anchor: pt(58.0, 20.0) };
        let end_port   = PortResult { port: pt(100.0, 20.0), anchor: pt(82.0, 20.0) };

        let r = f.grid_route_impl(&no_rel(), "A", "B", "right", "left", 0.0, &[], &start_port, &end_port);
        let r = r.expect("expected Some route");
        assert_eq!(r.d, "M 58 20 L 82 20");
        assert_eq!(r.points, vec![pt(58.0, 20.0), pt(82.0, 20.0)]);
    }

    #[test]
    fn grid_route_diagonal_four_bends() {
        // Node golden: diagonal arrangement → 4-bend route
        // d: "M 68 25 L 50 25 L 50 30 L 200 30 L 200 125 L 182 125"
        // bends: 4
        let from_rect = rect(0.0, 0.0, 50.0, 50.0);
        let to_rect = rect(200.0, 100.0, 50.0, 50.0);
        let f = make_factory(from_rect, to_rect, 400.0, 300.0, None, vec![]);

        let start_port = PortResult { port: pt(50.0, 25.0), anchor: pt(68.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 125.0), anchor: pt(182.0, 125.0) };

        let r = f.grid_route_impl(&no_rel(), "A", "B", "right", "left", 0.0, &[], &start_port, &end_port);
        let r = r.expect("expected Some route");
        assert_eq!(r.bends, 4);
        assert_eq!(
            r.d,
            "M 68 25 L 50 25 L 50 30 L 200 30 L 200 125 L 182 125"
        );
    }

    #[test]
    fn grid_route_budget_bailout_returns_none() {
        // Node golden: maxPoints=10, 50 blockers → null (budget bailout)
        // stats.gridRouteBudgetBailouts == 1
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(500.0, 0.0, 100.0, 50.0);
        let many_blockers: Vec<Rect> = (0..50)
            .map(|i| rect(i as f64 * 60.0, i as f64 * 30.0, 30.0, 20.0))
            .collect();

        let from_clone = from_rect.clone();
        let to_clone = to_rect.clone();
        let blockers_clone = many_blockers.clone();
        let mut f = RouteCandidateBuilders::new(
            move |_, _| blockers_clone.clone(),
            3000.0,
            3000.0,
            None,
            None,
            Some(10), // tiny max_points to trigger bailout
            move |id: &str| {
                if id == "A" { from_clone.clone() } else { to_clone.clone() }
            },
            NoopQuality,
            Some(BuilderStats::default()),
            None,
        );

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(500.0, 25.0), anchor: pt(482.0, 25.0) };
        let r = f.grid_route_impl(&no_rel(), "A", "B", "right", "left", 0.0, &[], &start_port, &end_port);
        assert!(r.is_none());
        let stats_ref = f.stats.borrow();
        let s = stats_ref.as_ref().unwrap();
        assert_eq!(s.grid_route_calls, 1);
        assert_eq!(s.grid_route_budget_bailouts, 1);
    }

    // -----------------------------------------------------------------------
    // splineCandidate — golden from Node
    // -----------------------------------------------------------------------

    #[test]
    fn spline_candidate_d_string() {
        // Node golden:
        // d: "M 118 25 C 182 61 118 61 182 25"
        // bends: 0, labelY: 51.99999999999999
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let sp = f
            .spline_candidate_impl(&no_rel(), "A", "B", "right", "left", &[], &start_port, &end_port, 0, 36.0)
            .expect("expected Some spline");

        assert_eq!(sp.d, "M 118 25 C 182 61 118 61 182 25");
        assert_eq!(sp.bends, 0);
        assert_eq!(sp.label_x, 150.0);
        // Node: labelY = 51.99999999999999 (IEEE double)
        assert!((sp.label_y - 51.99999999999999_f64).abs() < 1e-9);
        assert_eq!(sp.points, vec![pt(118.0, 25.0), pt(182.0, 25.0)]);
    }

    // -----------------------------------------------------------------------
    // straightCandidate — golden from Node
    // -----------------------------------------------------------------------

    #[test]
    fn straight_candidate_d_and_label() {
        // Node golden: d: "M 118 25 L 182 25", cost: -2115.5555555555557
        // labelX: 153.55555555555554, labelY: 3 (short route → horizontal → labelY -=22 → 25-22=3)
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let sc = f
            .straight_candidate_impl(&no_rel(), "A", "B", "right", "left", &[], &start_port, &end_port)
            .expect("expected Some straight");

        assert_eq!(sc.d, "M 118 25 L 182 25");
        // labelY = 25 - 22 = 3 (short horizontal → readable label shift)
        assert_eq!(sc.label_y, 3.0);
        assert!((sc.label_x - 153.555_555_555_555_54_f64).abs() < 1e-9);
    }

    // -----------------------------------------------------------------------
    // directPortCandidate
    // -----------------------------------------------------------------------

    #[test]
    fn direct_port_candidate_same_y_no_blockers() {
        // Node golden: d: "M 118 25 L 182 25" (horizontal line, same y)
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let dp = f
            .direct_port_candidate_impl(&no_rel(), "A", "B", "right", "left", &[], &start_port, &end_port)
            .expect("expected Some directPort");
        assert_eq!(dp.d, "M 118 25 L 182 25");
    }

    #[test]
    fn direct_port_candidate_different_y_returns_none() {
        // Different y → not horizontal or vertical → None
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 100.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 125.0), anchor: pt(182.0, 125.0) };

        let dp = f.direct_port_candidate_impl(
            &no_rel(), "A", "B", "right", "left", &[], &start_port, &end_port,
        );
        assert!(dp.is_none());
    }

    // -----------------------------------------------------------------------
    // perimeterRoute — golden from Node
    // -----------------------------------------------------------------------

    #[test]
    fn perimeter_route_bottom_no_node_bounds() {
        // Node golden (no nodeBounds):
        // d: "M 118 25 L 100 25 L 100 376 L 200 376 L 200 25 L 182 25"
        // bottom gutter = canvasHeight - CANVAS_INSET.bottom = 400 - 24 = 376
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let pr = f
            .perimeter_route_impl(&no_rel(), "A", "B", "bottom", 0.0, &[], &start_port, &end_port)
            .expect("expected Some perimeter");
        assert_eq!(pr.d, "M 118 25 L 100 25 L 100 376 L 200 376 L 200 25 L 182 25");
    }

    #[test]
    fn perimeter_route_bottom_with_node_bounds() {
        // Node golden (nodeBounds present): bottom = min(400-24, 50+24) = 74
        // d: "M 118 25 L 100 25 L 100 74 L 200 74 L 200 25 L 182 25"
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(
            from_rect,
            to_rect,
            600.0,
            400.0,
            Some(NodeBounds { min_x: 0.0, min_y: 0.0, max_x: 300.0, max_y: 50.0 }),
            vec![],
        );

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let pr = f
            .perimeter_route_impl(&no_rel(), "A", "B", "bottom", 0.0, &[], &start_port, &end_port)
            .expect("expected Some perimeter");
        assert_eq!(pr.d, "M 118 25 L 100 25 L 100 74 L 200 74 L 200 25 L 182 25");
    }

    // -----------------------------------------------------------------------
    // corridorCandidate
    // -----------------------------------------------------------------------

    #[test]
    fn corridor_candidate_x_axis_collapses_to_direct() {
        // Node golden: corridor at x=150, but simplified → direct line
        // d: "M 118 25 L 182 25"
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let cc = f
            .corridor_candidate_impl(
                &no_rel(), "A", "B", "right", "left", &[], &start_port, &end_port, "x", 150.0,
            )
            .expect("expected Some corridor");
        assert_eq!(cc.d, "M 118 25 L 182 25");
        assert_eq!(cc.points, vec![pt(118.0, 25.0), pt(182.0, 25.0)]);
    }

    #[test]
    fn corridor_candidate_blocked_returns_none() {
        // A blocker that intersects the corridor path → None
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        // Blocker covers the corridor line at x=150 between y=0 and y=60
        let blocker = rect(140.0, 10.0, 20.0, 40.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![blocker]);

        // Use y=35 so the corridor line actually goes through the blocker
        let start_port = PortResult { port: pt(100.0, 35.0), anchor: pt(118.0, 35.0) };
        let end_port   = PortResult { port: pt(200.0, 15.0), anchor: pt(182.0, 15.0) };

        let cc = f.corridor_candidate_impl(
            &no_rel(), "A", "B", "right", "left", &[], &start_port, &end_port, "x", 150.0,
        );
        assert!(cc.is_none());
    }

    // -----------------------------------------------------------------------
    // cornerPerimeterRoutes
    // -----------------------------------------------------------------------

    #[test]
    fn corner_perimeter_routes_count() {
        // Node golden: 4 corners × 2 paths each = 8 routes
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let start_port = PortResult { port: pt(100.0, 25.0), anchor: pt(118.0, 25.0) };
        let end_port   = PortResult { port: pt(200.0, 25.0), anchor: pt(182.0, 25.0) };

        let cprs = f.corner_perimeter_routes_impl(
            &no_rel(), "A", "B", 0.0, &[], &start_port, &end_port,
        );
        // Node golden: 8 routes
        assert_eq!(cprs.len(), 8);
        // Node golden index 1: "M 118 25 L 100 25 L 100 30 L 200 30 L 200 25 L 182 25"
        assert_eq!(cprs[1].d, "M 118 25 L 100 25 L 100 30 L 200 30 L 200 25 L 182 25");
        // Node golden index 5: "M 118 25 L 100 25 L 100 376 L 200 376 L 200 25 L 182 25"
        assert_eq!(cprs[5].d, "M 118 25 L 100 25 L 100 376 L 200 376 L 200 25 L 182 25");
    }

    // -----------------------------------------------------------------------
    // RouteCandidateFactory trait delegation
    // -----------------------------------------------------------------------

    #[test]
    fn trait_direct_port_candidate_delegates() {
        // Sanity check: trait impl delegates to the _impl method correctly.
        let from_rect = rect(0.0, 0.0, 100.0, 50.0);
        let to_rect = rect(200.0, 0.0, 100.0, 50.0);
        let f = make_factory(from_rect, to_rect, 600.0, 400.0, None, vec![]);

        let rel = no_rel();
        let r = f.direct_port_candidate(
            &rel, "A", "B", "right", "left", &[],
            &pt(118.0, 25.0), &pt(100.0, 25.0),
            &pt(182.0, 25.0), &pt(200.0, 25.0),
        );
        assert!(r.is_some());
        assert_eq!(r.unwrap().d, "M 118 25 L 182 25");
    }
}
