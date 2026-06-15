//! Faithful port of `viewer/src/routing/routeIndex.js`.
//!
//! Translation decisions:
//! - `RouteIndex` owns four collections: `horizontal` (Vec of HSegment), `vertical`
//!   (Vec of VSegment), `start_points` (IndexSet<String>), `end_points` (IndexSet<String>).
//!   `IndexSet` preserves insertion order for point dedup, matching JS `Set` semantics.
//! - `add(route, route_index)`: iterates over consecutive point pairs; pushes to
//!   `horizontal` or `vertical` based on which axis is constant. Skips routes with no
//!   points. This is the ordering that feeds `crossing_stats` and `shared_segment_stats`.
//! - `crossing_stats(points)`: for each horizontal segment of `points`, scans all
//!   `vertical` segments in the index; for each vertical segment of `points`, scans all
//!   `horizontal` segments. Counts unique routes crossed (by `route_index`) and total/
//!   repeated crossings. Replaces the JS `Map<routeIndex, count>` with `IndexMap`.
//! - `shared_segment_stats(points)`: segments of `points` against all same-axis segments
//!   in the index. Overlap > 1.0 counts. Counts self-overlap (segment vs itself when
//!   the route was added) — this is the JS behavior confirmed by Node testing.
//! - `has_stacked_endpoint(route)`: checks start/end against the global startPoints/
//!   endPoints sets. Returns false for empty/null routes.
//! - `adjacent_corridors(from_rect, to_rect, spacing)`: generates corridor candidates
//!   adjacent to existing route segments. Deduplication uses `IndexSet<String>` with
//!   `"axis:value"` keys, matching JS insertion-order dedup semantics.
//! - All comparisons use f64, matching JS number arithmetic.
//! - `HOP_RADIUS` is imported from `route_rendering` (= 6.0), matching JS import.

use indexmap::{IndexMap, IndexSet};

use crate::model::{Point, Rect};
use crate::route_rendering::HOP_RADIUS;

// ---------------------------------------------------------------------------
// Segment types
// ---------------------------------------------------------------------------

/// A horizontal segment stored in the index.
#[derive(Debug, Clone)]
pub struct HSegment {
    pub route_index: usize,
    pub y: f64,
    pub min_x: f64,
    pub max_x: f64,
    pub start: Point,
    pub end: Point,
}

/// A vertical segment stored in the index.
#[derive(Debug, Clone)]
pub struct VSegment {
    pub route_index: usize,
    pub x: f64,
    pub min_y: f64,
    pub max_y: f64,
    pub start: Point,
    pub end: Point,
}

// ---------------------------------------------------------------------------
// RouteIndex
// ---------------------------------------------------------------------------

/// A spatial index over routes. Created once per routing pass and queried by
/// `scoreRouteCandidates` for crossing/collision statistics.
///
/// Port of the object returned by JS `createRouteIndex()`.
pub struct RouteIndex {
    horizontal: Vec<HSegment>,
    vertical: Vec<VSegment>,
    start_points: IndexSet<String>,
    end_points: IndexSet<String>,
}

impl RouteIndex {
    /// Port of JS `createRouteIndex()`.
    pub fn new() -> Self {
        Self {
            horizontal: Vec::new(),
            vertical: Vec::new(),
            start_points: IndexSet::new(),
            end_points: IndexSet::new(),
        }
    }

    // -----------------------------------------------------------------------
    // add
    // -----------------------------------------------------------------------

    /// Port of JS `add(route, routeIndex)`.
    ///
    /// Indexes all axis-aligned segments of `route`. Each horizontal segment is
    /// pushed to `horizontal`, each vertical segment to `vertical`.
    /// The start/end points are added to the point sets.
    pub fn add(&mut self, points: &[Point], route_index: usize) {
        if points.is_empty() {
            return;
        }
        // JS: startPoints.add(`${route.points[0].x},${route.points[0].y}`)
        let first = &points[0];
        self.start_points.insert(format!("{},{}", first.x, first.y));
        let last = &points[points.len() - 1];
        self.end_points.insert(format!("{},{}", last.x, last.y));

        for index in 0..points.len() - 1 {
            let start = &points[index];
            let end = &points[index + 1];
            if start.y == end.y {
                self.horizontal.push(HSegment {
                    route_index,
                    y: start.y,
                    min_x: f64::min(start.x, end.x),
                    max_x: f64::max(start.x, end.x),
                    start: start.clone(),
                    end: end.clone(),
                });
            } else if start.x == end.x {
                self.vertical.push(VSegment {
                    route_index,
                    x: start.x,
                    min_y: f64::min(start.y, end.y),
                    max_y: f64::max(start.y, end.y),
                    start: start.clone(),
                    end: end.clone(),
                });
            }
        }
    }

    // -----------------------------------------------------------------------
    // crossing_stats
    // -----------------------------------------------------------------------

    /// Port of JS `crossingStats(points)`.
    ///
    /// Returns `{ total, repeated }` crossing counts for `points` against the
    /// indexed routes. Uses `HOP_RADIUS` as the interior margin (matching JS).
    ///
    /// The JS `counts` Map is replicated with `IndexMap<usize, i64>` to preserve
    /// insertion order (though order doesn't affect the numeric totals here).
    pub fn crossing_stats(&self, points: &[Point]) -> CrossingStats {
        let mut counts: IndexMap<usize, i64> = IndexMap::new();

        for index in 0..points.len().saturating_sub(1) {
            let start = &points[index];
            let end = &points[index + 1];
            // Skip if neither horizontal nor vertical
            if start.x != end.x && start.y != end.y {
                continue;
            }

            if start.y == end.y {
                // Horizontal segment — check against all vertical indexed segments
                let min_x = f64::min(start.x, end.x);
                let max_x = f64::max(start.x, end.x);
                for seg in &self.vertical {
                    if seg.x > min_x + HOP_RADIUS
                        && seg.x < max_x - HOP_RADIUS
                        && start.y > seg.min_y + HOP_RADIUS
                        && start.y < seg.max_y - HOP_RADIUS
                    {
                        *counts.entry(seg.route_index).or_insert(0) += 1;
                    }
                }
            } else {
                // Vertical segment — check against all horizontal indexed segments
                let min_y = f64::min(start.y, end.y);
                let max_y = f64::max(start.y, end.y);
                for seg in &self.horizontal {
                    if start.x > seg.min_x + HOP_RADIUS
                        && start.x < seg.max_x - HOP_RADIUS
                        && seg.y > min_y + HOP_RADIUS
                        && seg.y < max_y - HOP_RADIUS
                    {
                        *counts.entry(seg.route_index).or_insert(0) += 1;
                    }
                }
            }
        }

        // JS: const total = [...counts.values()].reduce((sum, count) => sum + count, 0);
        // JS: const repeated = [...counts.values()].reduce((sum, count) => sum + Math.max(0, count - 1), 0);
        let total: i64 = counts.values().sum();
        let repeated: i64 = counts.values().map(|&c| i64::max(0, c - 1)).sum();
        CrossingStats { total, repeated }
    }

    // -----------------------------------------------------------------------
    // shared_segment_stats
    // -----------------------------------------------------------------------

    /// Port of JS `sharedSegmentStats(points)`.
    ///
    /// Returns `{ count, length }` of overlapping segments between `points` and
    /// all indexed segments on the same axis and same position. Overlap > 1.0
    /// triggers a count. Self-overlap is included (the JS index includes the route
    /// being scored, and `sharedSegmentStats` does not exclude it).
    pub fn shared_segment_stats(&self, points: &[Point]) -> SharedSegmentStats {
        let mut count = 0i64;
        let mut length = 0.0f64;

        for index in 0..points.len().saturating_sub(1) {
            let start = &points[index];
            let end = &points[index + 1];
            if start.x != end.x && start.y != end.y {
                continue;
            }

            if start.y == end.y {
                // Horizontal
                let min_x = f64::min(start.x, end.x);
                let max_x = f64::max(start.x, end.x);
                for seg in &self.horizontal {
                    if seg.y != start.y {
                        continue;
                    }
                    let overlap = f64::min(max_x, seg.max_x) - f64::max(min_x, seg.min_x);
                    if overlap > 1.0 {
                        count += 1;
                        length += overlap;
                    }
                }
            } else {
                // Vertical
                let min_y = f64::min(start.y, end.y);
                let max_y = f64::max(start.y, end.y);
                for seg in &self.vertical {
                    if seg.x != start.x {
                        continue;
                    }
                    let overlap = f64::min(max_y, seg.max_y) - f64::max(min_y, seg.min_y);
                    if overlap > 1.0 {
                        count += 1;
                        length += overlap;
                    }
                }
            }
        }

        SharedSegmentStats { count, length }
    }

    // -----------------------------------------------------------------------
    // has_stacked_endpoint
    // -----------------------------------------------------------------------

    /// Port of JS `hasStackedEndpoint(route)`.
    ///
    /// Returns `true` if the route's start or end point is already in the
    /// global point sets. Returns `false` for empty routes.
    pub fn has_stacked_endpoint(&self, points: &[Point]) -> bool {
        if points.is_empty() {
            return false;
        }
        let start = &points[0];
        let end = &points[points.len() - 1];
        self.start_points.contains(&format!("{},{}", start.x, start.y))
            || self.end_points.contains(&format!("{},{}", end.x, end.y))
    }

    // -----------------------------------------------------------------------
    // adjacent_corridors
    // -----------------------------------------------------------------------

    /// Port of JS `adjacentCorridors(fromRect, toRect, spacing = 12)`.
    ///
    /// Returns candidate corridor offsets adjacent to existing route segments.
    /// Deduplicates by `"axis:value"` key in insertion order (matching JS `Set`).
    ///
    /// The JS offsets are `[1,2,3,4].map(m => spacing * m)` = `[12,24,36,48]`.
    /// For each indexed segment, two corridor candidates are generated per offset:
    /// `segment.x ± offset` for vertical segments, `segment.y ± offset` for horizontal.
    pub fn adjacent_corridors(
        &self,
        from_rect: &Rect,
        to_rect: &Rect,
        spacing: f64,
    ) -> Vec<Corridor> {
        let offsets = [1.0, 2.0, 3.0, 4.0].map(|m| spacing * m);
        let min_x = f64::min(from_rect.x, to_rect.x) - spacing * 6.0;
        let max_x = f64::max(from_rect.x + from_rect.width, to_rect.x + to_rect.width) + spacing * 6.0;
        let min_y = f64::min(from_rect.y, to_rect.y) - spacing * 6.0;
        let max_y = f64::max(from_rect.y + from_rect.height, to_rect.y + to_rect.height) + spacing * 6.0;

        let mut corridors: Vec<Corridor> = Vec::new();

        // Check vertical indexed segments
        for seg in &self.vertical {
            if seg.max_y < min_y || seg.min_y > max_y {
                continue;
            }
            for &offset in &offsets {
                for value in [seg.x - offset, seg.x + offset] {
                    if value >= min_x && value <= max_x {
                        corridors.push(Corridor { axis: CorridorAxis::X, value });
                    }
                }
            }
        }

        // Check horizontal indexed segments
        for seg in &self.horizontal {
            if seg.max_x < min_x || seg.min_x > max_x {
                continue;
            }
            for &offset in &offsets {
                for value in [seg.y - offset, seg.y + offset] {
                    if value >= min_y && value <= max_y {
                        corridors.push(Corridor { axis: CorridorAxis::Y, value });
                    }
                }
            }
        }

        // Deduplicate by "axis:value" key, preserving insertion order (JS Set semantics)
        let mut seen: IndexSet<String> = IndexSet::new();
        corridors
            .into_iter()
            .filter(|c| {
                let key = format!("{}:{}", c.axis.as_str(), c.value);
                seen.insert(key)
            })
            .collect()
    }
}

impl Default for RouteIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct CrossingStats {
    pub total: i64,
    pub repeated: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharedSegmentStats {
    pub count: i64,
    pub length: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Corridor {
    pub axis: CorridorAxis,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorridorAxis {
    X,
    Y,
}

impl CorridorAxis {
    fn as_str(&self) -> &'static str {
        match self {
            CorridorAxis::X => "x",
            CorridorAxis::Y => "y",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    // -----------------------------------------------------------------------
    // crossing_stats
    // -----------------------------------------------------------------------

    #[test]
    fn crossing_stats_basic() {
        // Node: route0 horizontal (0,50)→(200,50), route1 vertical (100,0)→(100,100)
        // route2 horizontal (0,80)→(200,80)
        // crossingStats(route2.points) → { total:1, repeated:0 } (crosses route1)
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route1 = vec![p(100.0, 0.0), p(100.0, 100.0)];
        let route2 = vec![p(0.0, 80.0), p(200.0, 80.0)];
        idx.add(&route0, 0);
        idx.add(&route1, 1);
        idx.add(&route2, 2);

        let stats2 = idx.crossing_stats(&route2);
        assert_eq!(stats2, CrossingStats { total: 1, repeated: 0 });
    }

    #[test]
    fn crossing_stats_route0() {
        // Node: route0 horizontal y=50 crosses route1 vertical x=100 → { total:1, repeated:0 }
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route1 = vec![p(100.0, 0.0), p(100.0, 100.0)];
        idx.add(&route0, 0);
        idx.add(&route1, 1);

        let stats0 = idx.crossing_stats(&route0);
        assert_eq!(stats0, CrossingStats { total: 1, repeated: 0 });
    }

    #[test]
    fn crossing_stats_no_crossings() {
        // Two horizontal routes at same y don't cross each other
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route1 = vec![p(0.0, 80.0), p(200.0, 80.0)];
        idx.add(&route0, 0);
        idx.add(&route1, 1);

        let stats = idx.crossing_stats(&route0);
        assert_eq!(stats, CrossingStats { total: 0, repeated: 0 });
    }

    #[test]
    fn crossing_stats_repeated() {
        // A route that crosses the same indexed route twice → repeated=1
        // Route A: L-shape that crosses a vertical segment twice
        // A: (0,50)→(200,50)→(200,150)→(0,150) — two horizontal segments both crossing x=100
        let mut idx = RouteIndex::new();
        let vertical = vec![p(100.0, 0.0), p(100.0, 200.0)];
        idx.add(&vertical, 0);

        // Route B: two horizontal segments both crossing x=100
        let route_b = vec![p(0.0, 50.0), p(200.0, 50.0), p(200.0, 150.0), p(0.0, 150.0)];
        let stats = idx.crossing_stats(&route_b);
        // Both horizontal segments cross route0 → counts[0] = 2 → total=2, repeated=1
        assert_eq!(stats.total, 2);
        assert_eq!(stats.repeated, 1);
    }

    // -----------------------------------------------------------------------
    // shared_segment_stats
    // -----------------------------------------------------------------------

    #[test]
    fn shared_segment_stats_self_overlap() {
        // Node: route0 added; sharedSegmentStats(route0.points) → {count:1, length:200}
        // (route0 overlaps with itself in the index)
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        idx.add(&route0, 0);

        let stats = idx.shared_segment_stats(&route0);
        assert_eq!(stats, SharedSegmentStats { count: 1, length: 200.0 });
    }

    #[test]
    fn shared_segment_stats_different_y() {
        // Two horizontal routes at different y → no shared segment overlap
        // But self-overlap counts for route0 vs itself
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route2 = vec![p(0.0, 80.0), p(200.0, 80.0)];
        idx.add(&route0, 0);
        idx.add(&route2, 2);

        // route0 checks vs all horizontal; only route0 is at y=50 → count=1, length=200
        let stats = idx.shared_segment_stats(&route0);
        assert_eq!(stats, SharedSegmentStats { count: 1, length: 200.0 });
    }

    #[test]
    fn shared_segment_stats_partial_overlap() {
        // Node: route0 (0..200) and route3 (50..150) at same y=50
        // Overlap = min(200,150) - max(0,50) = 100
        // With route0 also being in index: route0 checks vs route0 (200) and route3 (100) → count=2, length=300
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        let route3 = vec![p(50.0, 50.0), p(150.0, 50.0)];
        idx.add(&route0, 0);
        idx.add(&route3, 3);

        let stats = idx.shared_segment_stats(&route0);
        assert_eq!(stats, SharedSegmentStats { count: 2, length: 300.0 });
    }

    // -----------------------------------------------------------------------
    // has_stacked_endpoint
    // -----------------------------------------------------------------------

    #[test]
    fn has_stacked_endpoint_same_start() {
        // Node: route with start (0,50) same as route0's start → true
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        idx.add(&route0, 0);

        let route_same_start = vec![p(0.0, 50.0), p(0.0, 100.0)];
        assert!(idx.has_stacked_endpoint(&route_same_start));
    }

    #[test]
    fn has_stacked_endpoint_fresh() {
        // Node: route with fresh (999,999) start → false
        let mut idx = RouteIndex::new();
        let route0 = vec![p(0.0, 50.0), p(200.0, 50.0)];
        idx.add(&route0, 0);

        let fresh = vec![p(999.0, 999.0), p(999.0, 100.0)];
        assert!(!idx.has_stacked_endpoint(&fresh));
    }

    #[test]
    fn has_stacked_endpoint_empty() {
        // Node: empty route → false
        let idx = RouteIndex::new();
        assert!(!idx.has_stacked_endpoint(&[]));
    }

    // -----------------------------------------------------------------------
    // adjacent_corridors
    // -----------------------------------------------------------------------

    #[test]
    fn adjacent_corridors_vertical_segment() {
        // Node: vertical segment at x=100; adjacentCorridors produces x-axis corridors
        // at 100±12, 100±24, 100±36, 100±48 in insertion order: 88, 112, 76, 124, 64, 136, 52, 148
        let mut idx = RouteIndex::new();
        let vertical = vec![p(100.0, 0.0), p(100.0, 100.0)];
        idx.add(&vertical, 0);

        let from_rect = rect(0.0, 0.0, 80.0, 50.0);
        let to_rect = rect(150.0, 0.0, 80.0, 50.0);
        let corridors = idx.adjacent_corridors(&from_rect, &to_rect, 12.0);

        let values: Vec<f64> = corridors.iter().map(|c| c.value).collect();
        assert_eq!(values, vec![88.0, 112.0, 76.0, 124.0, 64.0, 136.0, 52.0, 148.0]);
        assert!(corridors.iter().all(|c| c.axis == CorridorAxis::X));
    }

    #[test]
    fn adjacent_corridors_dedup() {
        // Adding a second vertical segment at x=100 should not produce duplicates
        let mut idx = RouteIndex::new();
        idx.add(&[p(100.0, 0.0), p(100.0, 100.0)], 0);
        idx.add(&[p(100.0, 10.0), p(100.0, 80.0)], 1);

        let from_rect = rect(0.0, 0.0, 80.0, 50.0);
        let to_rect = rect(150.0, 0.0, 80.0, 50.0);
        let corridors = idx.adjacent_corridors(&from_rect, &to_rect, 12.0);
        // Same 8 corridors as without the duplicate
        assert_eq!(corridors.len(), 8);
    }

    #[test]
    fn adjacent_corridors_horizontal_segment() {
        // Horizontal segment at y=50; should produce y-axis corridors
        let mut idx = RouteIndex::new();
        idx.add(&[p(0.0, 50.0), p(200.0, 50.0)], 0);

        let from_rect = rect(0.0, 0.0, 100.0, 30.0);
        let to_rect = rect(200.0, 60.0, 100.0, 30.0);
        let corridors = idx.adjacent_corridors(&from_rect, &to_rect, 12.0);
        // All corridors should be y-axis
        assert!(corridors.iter().all(|c| c.axis == CorridorAxis::Y));
        // y=50 ± 12, 24, 36, 48 = 38, 62, 26, 74, 14, 86, 2, 98
        let values: Vec<f64> = corridors.iter().map(|c| c.value).collect();
        assert_eq!(values, vec![38.0, 62.0, 26.0, 74.0, 14.0, 86.0, 2.0, 98.0]);
    }
}
