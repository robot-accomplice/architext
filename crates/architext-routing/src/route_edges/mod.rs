//! Faithful port of `viewer/src/routing/routeEdges.js`.
//!
//! Split into cohesive submodules; all public items are re-exported here so
//! external code using `crate::route_edges::X` paths keeps working unchanged.
//!
//! Submodule responsibilities:
//! - `types` — `RouteData`, `RouteInput`, `Relationship`, `AxisAlignedSegment`.
//! - `helpers` — geometry helpers, collision checks, route-point construction, offset/polyline utilities, shared-segment rendering (Pass A + C1 L54–L129).
//! - `construction` — C1 cleanup helpers: `EndpointSideUsage`, `recentered_*`, `RelationshipC1`, `RouteInputC1`, aligned/cleanup route fns, endpoint stub enforcement.
//! - `separation` — C2 parallel separation + C3 `spread_unit_slots`.

pub mod types;
pub mod helpers;
pub mod construction;
pub mod separation;
pub mod crossings;
pub mod orchestration;

// ---------------------------------------------------------------------------
// Re-export the complete public surface so `crate::route_edges::X` resolves.
// ---------------------------------------------------------------------------

// -- types --
pub use types::{AxisAlignedSegment, Relationship, RouteData, RouteInput};

// -- helpers --
pub use helpers::{
    axis_aligned_segments, endpoint_offset_points, endpoint_side, endpoint_spread_offset,
    final_shared_segment_stats, offset_endpoint_route, offset_orthogonal_polyline,
    recentered_endpoint_points_with_anchors, render_orthogonal_route,
    rendered_axis_aligned_segments, route_collides_with_non_endpoints,
    route_has_endpoint_traversal, route_with_points, shared_segment_length, side_endpoint_key,
    side_needs_post_selection_centering, SharedSegmentStats,
};

// -- construction --
pub use construction::{
    aligned_facing_endpoint_route, aligned_fixed_port_route,
    collapse_aligned_opposing_surface_route, endpoint_stub_route, enforce_endpoint_stubs,
    non_endpoint_node_collision_count, recentered_endpoint_points, recentered_endpoint_route,
    recentered_endpoint_route_with_anchors, recentered_without_new_shared_segments,
    route_with_best_cleanup_candidate, route_with_endpoint_stubs,
    route_with_fewest_shared_segments, shared_segment_count,
    EndpointSideUsage, PlanRelationship, RelationshipC1, RouteInputC1,
};

// -- crossings --
pub use crossings::{crossings_between, crossings_involving, gutter_lane_of};

// -- orchestration --
pub use orchestration::{
    route_edges, route_edges_with_stats, route_planner_context, CorpusPlanStats, InputRelationship,
    NodeRect, PlannerContext, RouteEdgesInput, RouteQualityImpl,
};

// -- separation --
pub use separation::{
    alternate_middle_dogleg_routes, axis_aligned_route_segments, close_parallel_run_count_between,
    close_parallel_run_count_for_routes, close_parallel_segment_pair, crossing_pair_key,
    is_better_route_separation, is_better_route_set, route_endpoints_are_perpendicular,
    route_pair_index, route_separation_score, route_set_score, route_set_stats,
    separate_close_parallel_routes, shifted_direct_endpoint_route, shifted_endpoint_segment_route,
    shifted_internal_segment_route, spread_unit_slots, total_bends_for_routes,
    CloseParallelPair, NoopReroute, RerouteCallback, RouteSegment, RouteSetStats,
    SeparationRelationship, ROUTE_SEPARATION_DISTANCES,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Rect};
    use indexmap::IndexMap;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    /// Minimal RouteData for testing (orthogonal style with integer coordinates
    /// so the sample-bounds / d comparisons are simple).
    fn orthogonal_route(points: Vec<Point>) -> RouteData {
        let samples = crate::route_geometry::line_samples(&points);
        let sb = crate::route_geometry::bounds_for_points(&{
            let mut v = points.clone();
            v.extend(samples.iter().cloned());
            v
        });
        let bends = crate::route_geometry::bend_count(&points);
        let label = samples.get(samples.len() / 2).cloned()
            .or_else(|| points.get(points.len() / 2).cloned())
            .unwrap_or(pt(0.0, 0.0));
        RouteData {
            d: {
                // Replicate build_ml_path locally for the test helper
                use crate::js_compat::js_number_to_string;
                points
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let cmd = if i == 0 { "M" } else { "L" };
                        format!("{} {} {}", cmd, js_number_to_string(p.x), js_number_to_string(p.y))
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            },
            points,
            controls: None,
            samples,
            sample_bounds: sb,
            bends,
            label_x: label.x,
            label_y: label.y,
            style: "orthogonal".into(),
            extra: IndexMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // endpointSide
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_side_all_four() {
        // Node: rect={x:10,y:20,w:80,h:40}
        // left={x:10,y:40}→"left", right={x:90,y:40}→"right"
        // top={x:50,y:20}→"top", bottom={x:50,y:60}→"bottom"
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_side(&r, &pt(10.0, 40.0)), "left");
        assert_eq!(endpoint_side(&r, &pt(90.0, 40.0)), "right");
        assert_eq!(endpoint_side(&r, &pt(50.0, 20.0)), "top");
        assert_eq!(endpoint_side(&r, &pt(50.0, 60.0)), "bottom");
    }

    #[test]
    fn endpoint_side_interior_returns_empty() {
        // Node: interior point {x:50,y:40} → ""
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_side(&r, &pt(50.0, 40.0)), "");
    }

    #[test]
    fn endpoint_side_corner_prefers_left_over_top() {
        // Top-left corner: x==rect.x is checked first → "left"
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_side(&r, &pt(10.0, 20.0)), "left");
    }

    // -----------------------------------------------------------------------
    // sideNeedsPostSelectionCentering
    // -----------------------------------------------------------------------

    #[test]
    fn side_centering_all_four_sides_true() {
        // Node: left/right/top/bottom → true; "" or "other" → false
        assert!(side_needs_post_selection_centering("left"));
        assert!(side_needs_post_selection_centering("right"));
        assert!(side_needs_post_selection_centering("top"));
        assert!(side_needs_post_selection_centering("bottom"));
        assert!(!side_needs_post_selection_centering(""));
        assert!(!side_needs_post_selection_centering("diagonal"));
    }

    // -----------------------------------------------------------------------
    // axisAlignedSegments
    // -----------------------------------------------------------------------

    #[test]
    fn axis_aligned_segments_l_shape() {
        // Node: points [{x:0,y:0},{x:100,y:0},{x:100,y:50}]
        // → [{orientation:"horizontal",line:0,min:0,max:100},
        //    {orientation:"vertical",line:100,min:0,max:50}]
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)]);
        let segs = axis_aligned_segments(&route);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 0.0);
        assert_eq!(segs[0].max, 100.0);
        assert_eq!(segs[1].orientation, "vertical");
        assert_eq!(segs[1].line, 100.0);
        assert_eq!(segs[1].min, 0.0);
        assert_eq!(segs[1].max, 50.0);
    }

    #[test]
    fn axis_aligned_segments_single_segment() {
        // Node: [{x:50,y:0},{x:150,y:0}] → [{orientation:"horizontal",line:0,min:50,max:150}]
        let route = orthogonal_route(vec![pt(50.0, 0.0), pt(150.0, 0.0)]);
        let segs = axis_aligned_segments(&route);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 50.0);
        assert_eq!(segs[0].max, 150.0);
    }

    #[test]
    fn axis_aligned_segments_empty_route() {
        let route = orthogonal_route(vec![]);
        assert_eq!(axis_aligned_segments(&route).len(), 0);
    }

    // -----------------------------------------------------------------------
    // sharedSegmentLength
    // -----------------------------------------------------------------------

    #[test]
    fn shared_segment_length_horizontal_overlap() {
        // Node: left=[horiz,y=0,min=0,max=100], right=[horiz,y=0,min=50,max=150]
        // overlap = min(100,150)-max(0,50) = 100-50 = 50
        let left = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 100.0 };
        let right = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 50.0, max: 150.0 };
        assert_eq!(shared_segment_length(&left, &right), 50.0);
    }

    #[test]
    fn shared_segment_length_different_orientation() {
        // Node: vertical vs horizontal → 0
        let v = AxisAlignedSegment { orientation: "vertical", line: 100.0, min: 0.0, max: 50.0 };
        let h = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 100.0 };
        assert_eq!(shared_segment_length(&v, &h), 0.0);
    }

    #[test]
    fn shared_segment_length_different_line() {
        // Node: both horizontal but y=0 vs y=10 → 0
        let a = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 150.0 };
        let b = AxisAlignedSegment { orientation: "horizontal", line: 10.0, min: 0.0, max: 150.0 };
        assert_eq!(shared_segment_length(&a, &b), 0.0);
    }

    #[test]
    fn shared_segment_length_no_overlap() {
        // Node: [0,100] vs [200,300] → 0
        let a = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 0.0, max: 100.0 };
        let b = AxisAlignedSegment { orientation: "horizontal", line: 0.0, min: 200.0, max: 300.0 };
        assert_eq!(shared_segment_length(&a, &b), 0.0);
    }

    // -----------------------------------------------------------------------
    // routeCollidesWithNonEndpoints
    // -----------------------------------------------------------------------

    #[test]
    fn route_collides_with_non_endpoints_through_blocker() {
        // Node: route through C (x=100..150,y=0..50); from=A,to=B
        // collides → true
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        node_rects.insert("C".to_string(), rect(100.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        // Route going through C (x=100..150)
        let route = orthogonal_route(vec![pt(50.0, 25.0), pt(125.0, 25.0), pt(200.0, 25.0)]);
        assert!(route_collides_with_non_endpoints(&route, &rel, &input));
    }

    #[test]
    fn route_collides_avoids_below() {
        // Node: route going under C → false
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        node_rects.insert("C".to_string(), rect(100.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        // Route going below C
        let route = orthogonal_route(vec![
            pt(50.0, 25.0),
            pt(50.0, 75.0),
            pt(200.0, 75.0),
            pt(200.0, 25.0),
        ]);
        assert!(!route_collides_with_non_endpoints(&route, &rel, &input));
    }

    // -----------------------------------------------------------------------
    // routeHasEndpointTraversal
    // -----------------------------------------------------------------------

    #[test]
    fn route_has_endpoint_traversal_inside_from() {
        // Node: a sample point strictly inside A → true
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        let mut route = orthogonal_route(vec![pt(50.0, 25.0), pt(200.0, 25.0)]);
        // Override samples with one strictly inside A
        route.samples = vec![pt(25.0, 25.0)];
        assert!(route_has_endpoint_traversal(&route, &rel, &input));
    }

    #[test]
    fn route_has_no_endpoint_traversal() {
        // Node: sample outside both endpoints → false
        let mut node_rects = IndexMap::new();
        node_rects.insert("A".to_string(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".to_string(), rect(200.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".to_string(), "B".to_string()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        let mut route = orthogonal_route(vec![pt(50.0, 25.0), pt(200.0, 25.0)]);
        route.samples = vec![pt(125.0, 75.0)];
        assert!(!route_has_endpoint_traversal(&route, &rel, &input));
    }

    // -----------------------------------------------------------------------
    // endpointSpreadOffset
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_spread_offset_left_side_three() {
        // Node: rect h=40, side=left, 3 edges
        // index=0 → (1/4-0.5)*40 = -10
        // index=1 → (2/4-0.5)*40 = 0
        // index=2 → (3/4-0.5)*40 = 10
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_spread_offset(0, 3, &r, "left"), -10.0);
        assert_eq!(endpoint_spread_offset(1, 3, &r, "left"), 0.0);
        assert_eq!(endpoint_spread_offset(2, 3, &r, "left"), 10.0);
    }

    #[test]
    fn endpoint_spread_offset_top_side_two() {
        // Node: rect w=80, side=top, 2 edges
        // index=0 → (1/3-0.5)*80 = -13.333...
        // index=1 → (2/3-0.5)*80 = 13.333...
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let v0 = endpoint_spread_offset(0, 2, &r, "top");
        let v1 = endpoint_spread_offset(1, 2, &r, "top");
        assert!((v0 - (-13.333_333_333_333_336)).abs() < 1e-9);
        assert!((v1 - 13.333_333_333_333_33).abs() < 1e-9);
    }

    #[test]
    fn endpoint_spread_offset_single_right() {
        // Node: 1 edge on right side (h=40) → (1/2-0.5)*40 = 0
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(endpoint_spread_offset(0, 1, &r, "right"), 0.0);
    }

    // -----------------------------------------------------------------------
    // endpointOffsetPoints (private, tested through offsetEndpointRoute)
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_offset_points_2pt_left_offset10() {
        // Node: pts=[{x:100,y:240},{x:200,y:240}], ep=0, rect={x:100,y:200,w:60,h:80},
        // side="left", rawOffset=10
        // → [{x:100,y:250},{x:82,y:250},{x:82,y:240},{x:200,y:240}]
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let pts = vec![pt(100.0, 240.0), pt(200.0, 240.0)];
        let result = endpoint_offset_points(&pts, 0, &r, "left", 10.0);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], pt(100.0, 250.0));
        assert_eq!(result[1], pt(82.0, 250.0));
        assert_eq!(result[2], pt(82.0, 240.0));
        assert_eq!(result[3], pt(200.0, 240.0));
    }

    #[test]
    fn endpoint_offset_points_2pt_left_offset0() {
        // Node: rawOffset=0 → anchor at center: rect.y+h/2=240
        // → [{x:100,y:240},{x:82,y:240},{x:82,y:240},{x:200,y:240}]
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let pts = vec![pt(100.0, 240.0), pt(200.0, 240.0)];
        let result = endpoint_offset_points(&pts, 0, &r, "left", 0.0);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], pt(100.0, 240.0));
        assert_eq!(result[1], pt(82.0, 240.0));
        assert_eq!(result[2], pt(82.0, 240.0));
        assert_eq!(result[3], pt(200.0, 240.0));
    }

    #[test]
    fn endpoint_offset_points_multipt_centering() {
        // Node: 4-pt route, centering (rawOffset=0)
        // pts=[{100,240},{82,240},{82,300},{200,300}], ep=0, left
        // → unchanged because anchor already at (100,240) and adjacentY=240
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let pts = vec![pt(100.0, 240.0), pt(82.0, 240.0), pt(82.0, 300.0), pt(200.0, 300.0)];
        let result = endpoint_offset_points(&pts, 0, &r, "left", 0.0);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], pt(100.0, 240.0));
        assert_eq!(result[1], pt(82.0, 240.0));
    }

    // -----------------------------------------------------------------------
    // offsetEndpointRoute
    // -----------------------------------------------------------------------

    #[test]
    fn offset_endpoint_route_2pt_left_10() {
        // Node: offset endpoint 0 on left by rawOffset=10 produces 4-pt route
        let r = rect(100.0, 200.0, 60.0, 80.0);
        let route = orthogonal_route(vec![pt(100.0, 240.0), pt(200.0, 240.0)]);
        let result = offset_endpoint_route(&route, 0, &r, "left", 10.0);
        assert_eq!(result.points[0], pt(100.0, 250.0));
        assert_eq!(result.points.len(), 4);
        assert_eq!(result.style, "orthogonal");
    }

    // -----------------------------------------------------------------------
    // routeWithPoints
    // -----------------------------------------------------------------------

    #[test]
    fn route_with_points_orthogonal_l_shape() {
        // Node: 4-pt orthogonal route, already simplified
        // d = "M 100 240 L 82 240 L 82 300 L 200 300"
        // bends = 2
        // lineSamples: 10 steps × 3 segments = 30 samples; mid=floor(30/2)=15
        // Segment 0 steps: t=0.1..1.0 on [{100,240}→{82,240}]
        // Segment 1 steps: t=0.1..1.0 on [{82,240}→{82,300}]
        //   samples[10]={82,240}, [15]={82,270} … wait sample[15] is step 6 of seg 1:
        //   82+0*6*(1/10-0)=82, 240+(300-240)*6/10=276 → {82,276}
        // Node: confirmed in Node.js: label={x:82,y:276}
        let route = orthogonal_route(vec![pt(0.0, 0.0)]);
        let pts = vec![pt(100.0, 240.0), pt(82.0, 240.0), pt(82.0, 300.0), pt(200.0, 300.0)];
        let result = route_with_points(&route, pts, None);
        assert_eq!(result.d, "M 100 240 L 82 240 L 82 300 L 200 300");
        assert_eq!(result.bends, 2);
        assert_eq!(result.label_x, 82.0);
        assert_eq!(result.label_y, 276.0); // Node: samples[15]={x:82,y:276}
    }

    #[test]
    fn route_with_points_orthogonal_2pt_straight() {
        // Node: 2-pt horizontal → bends=0
        // lineSamples: 10 steps on [{10,40}→{200,40}]; mid=floor(10/2)=5
        // step 6 (index 5): t=0.6, x=10+(200-10)*0.6=10+114=124 → {124,40}
        // Node: confirmed label={x:124,y:40}
        let route = orthogonal_route(vec![pt(0.0, 0.0)]);
        let pts = vec![pt(10.0, 40.0), pt(200.0, 40.0)];
        let result = route_with_points(&route, pts, None);
        assert_eq!(result.d, "M 10 40 L 200 40");
        assert_eq!(result.bends, 0);
        assert_eq!(result.label_x, 124.0); // Node: samples[5]={x:124,y:40}
        assert_eq!(result.label_y, 40.0);
    }

    // -----------------------------------------------------------------------
    // offsetOrthogonalPolyline
    // -----------------------------------------------------------------------

    #[test]
    fn offset_orthogonal_polyline_l_shape_delta10() {
        // Node: pts=[{0,0},{100,0},{100,50}], delta=10
        // → [{0,-10},{110,-10},{110,50}]
        let pts = vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)];
        let result = offset_orthogonal_polyline(&pts, 10.0);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], pt(0.0, -10.0));
        assert_eq!(result[1], pt(110.0, -10.0));
        assert_eq!(result[2], pt(110.0, 50.0));
    }

    #[test]
    fn offset_orthogonal_polyline_horizontal_delta5() {
        // Node: pts=[{0,0},{100,0}], delta=5 → [{0,-5},{100,-5}]
        let pts = vec![pt(0.0, 0.0), pt(100.0, 0.0)];
        let result = offset_orthogonal_polyline(&pts, 5.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], pt(0.0, -5.0));
        assert_eq!(result[1], pt(100.0, -5.0));
    }

    #[test]
    fn offset_orthogonal_polyline_vertical_then_horiz_delta12() {
        // Node: pts=[{0,0},{0,50},{100,50}], delta=12
        // → [{12,0},{12,38},{100,38}]
        let pts = vec![pt(0.0, 0.0), pt(0.0, 50.0), pt(100.0, 50.0)];
        let result = offset_orthogonal_polyline(&pts, 12.0);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], pt(12.0, 0.0));
        assert_eq!(result[1], pt(12.0, 38.0));
        assert_eq!(result[2], pt(100.0, 38.0));
    }

    #[test]
    fn offset_orthogonal_polyline_null_returns_empty() {
        // Node: empty points → empty result
        let result = offset_orthogonal_polyline(&[], 10.0);
        assert!(result.is_empty());
    }

    #[test]
    fn offset_orthogonal_polyline_single_point_unchanged() {
        // Node: single point → [point] unchanged
        let pts = vec![pt(0.0, 0.0)];
        let result = offset_orthogonal_polyline(&pts, 10.0);
        assert_eq!(result, pts);
    }

    // -----------------------------------------------------------------------
    // Pass C1 tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // sideEndpointKey
    // -----------------------------------------------------------------------

    #[test]
    fn side_endpoint_key_nul_separator() {
        // Node: sideEndpointKey('node-A','left') → charCodes include NUL at pos 6
        // confirmed: [110,111,100,101,45,65,0,108,101,102,116]
        let k = side_endpoint_key("node-A", "left");
        let bytes: Vec<u8> = k.bytes().collect();
        // NUL byte at position 6 (after "node-A")
        assert_eq!(bytes[6], 0, "separator must be NUL");
        assert_eq!(k, "node-A\0left");
    }

    #[test]
    fn side_endpoint_key_same_same() {
        // Node: sideEndpointKey('A','right') === sideEndpointKey('A','right')
        assert_eq!(side_endpoint_key("A", "right"), side_endpoint_key("A", "right"));
    }

    #[test]
    fn side_endpoint_key_same_diff() {
        // Node: sideEndpointKey('A','right') !== sideEndpointKey('A','left')
        assert_ne!(side_endpoint_key("A", "right"), side_endpoint_key("A", "left"));
    }

    // -----------------------------------------------------------------------
    // renderedAxisAlignedSegments
    // -----------------------------------------------------------------------

    #[test]
    fn rendered_axis_aligned_segments_l_shape() {
        // Node: [{x:0,y:0},{x:100,y:0},{x:100,y:50}]
        // → [{"orientation":"horizontal","line":0,"min":0,"max":100},
        //    {"orientation":"vertical","line":100,"min":0,"max":50}]
        let pts = vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)];
        let segs = rendered_axis_aligned_segments(&pts);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 0.0);
        assert_eq!(segs[0].max, 100.0);
        assert_eq!(segs[1].orientation, "vertical");
        assert_eq!(segs[1].line, 100.0);
        assert_eq!(segs[1].min, 0.0);
        assert_eq!(segs[1].max, 50.0);
    }

    #[test]
    fn rendered_axis_aligned_segments_vertical() {
        // Node: [{x:10,y:5},{x:10,y:80}]
        // → [{"orientation":"vertical","line":10,"min":5,"max":80}]
        let pts = vec![pt(10.0, 5.0), pt(10.0, 80.0)];
        let segs = rendered_axis_aligned_segments(&pts);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].orientation, "vertical");
        assert_eq!(segs[0].line, 10.0);
        assert_eq!(segs[0].min, 5.0);
        assert_eq!(segs[0].max, 80.0);
    }

    #[test]
    fn rendered_axis_aligned_segments_empty() {
        // Node: null → [], [] → []
        assert!(rendered_axis_aligned_segments(&[]).is_empty());
        assert!(rendered_axis_aligned_segments(&[pt(0.0, 0.0)]).is_empty());
    }

    // -----------------------------------------------------------------------
    // finalSharedSegmentStats
    // -----------------------------------------------------------------------

    #[test]
    fn final_shared_segment_stats_horizontal_overlap() {
        // Node: finalSharedSegmentStats(rA, [rA,rB,rC]) = {count:1, length:50}
        // rA=[{0,0}→{100,0}], rB=[{50,0}→{200,0}] overlap=50, rC=[{0,5}→{100,5}] diff line
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_b = orthogonal_route(vec![pt(50.0, 0.0), pt(200.0, 0.0)]);
        let route_c = orthogonal_route(vec![pt(0.0, 5.0), pt(100.0, 5.0)]);
        let all = vec![route_a.clone(), route_b, route_c];
        let stats = final_shared_segment_stats(&route_a, &all, 0);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.length, 50.0);
    }

    #[test]
    fn final_shared_segment_stats_no_overlap() {
        // Node: finalSharedSegmentStats(rA, [rA,rC]) = {count:0, length:0}
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_c = orthogonal_route(vec![pt(0.0, 5.0), pt(100.0, 5.0)]);
        let all = vec![route_a.clone(), route_c];
        let stats = final_shared_segment_stats(&route_a, &all, 0);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.length, 0.0);
    }

    #[test]
    fn final_shared_segment_stats_vertical_overlap() {
        // Node: finalSharedSegmentStats(rD, [rD,rE]) = {count:1, length:50}
        // rD=[{10,0}→{10,100}], rE=[{10,50}→{10,150}] overlap=50
        let route_d = orthogonal_route(vec![pt(10.0, 0.0), pt(10.0, 100.0)]);
        let route_e = orthogonal_route(vec![pt(10.0, 50.0), pt(10.0, 150.0)]);
        let all = vec![route_d.clone(), route_e];
        let stats = final_shared_segment_stats(&route_d, &all, 0);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.length, 50.0);
    }

    // -----------------------------------------------------------------------
    // sharedSegmentCount
    // -----------------------------------------------------------------------

    #[test]
    fn shared_segment_count_one_overlap() {
        // Node: sharedSegmentCount(rA, [rB]) = 1
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_b = orthogonal_route(vec![pt(50.0, 0.0), pt(200.0, 0.0)]);
        assert_eq!(shared_segment_count(&route_a, &[route_b]), 1);
    }

    #[test]
    fn shared_segment_count_no_overlap() {
        // Node: sharedSegmentCount(rA, [rC]) = 0  (different y)
        let route_a = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let route_c = orthogonal_route(vec![pt(0.0, 5.0), pt(100.0, 5.0)]);
        assert_eq!(shared_segment_count(&route_a, &[route_c]), 0);
    }

    #[test]
    fn shared_segment_count_vertical() {
        // Node: sharedSegmentCount(rD, [rE]) = 1
        let route_d = orthogonal_route(vec![pt(10.0, 0.0), pt(10.0, 100.0)]);
        let route_e = orthogonal_route(vec![pt(10.0, 50.0), pt(10.0, 150.0)]);
        assert_eq!(shared_segment_count(&route_d, &[route_e]), 1);
    }

    // -----------------------------------------------------------------------
    // nonEndpointNodeCollisionCount
    // -----------------------------------------------------------------------

    #[test]
    fn non_endpoint_node_collision_count_skip_endpoints() {
        // Route passes through a node that is neither from nor to → count=1.
        // Route from A(0,0,50,50) to B(200,0,50,50) passes through C(100,0,20,20).
        let route = orthogonal_route(vec![pt(50.0, 10.0), pt(110.0, 10.0), pt(250.0, 10.0)]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 50.0, 50.0));
        // C sits at x=90..110, y=0..20; the route crosses through it at y=10
        node_rects.insert("C".into(), rect(90.0, 0.0, 20.0, 20.0));
        let visible = vec!["A".into(), "B".into(), "C".into()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        let count = non_endpoint_node_collision_count(&route, &rel, &input);
        assert_eq!(count, 1);
    }

    #[test]
    fn non_endpoint_node_collision_count_endpoints_skipped() {
        // Route only goes through its own endpoint nodes → count=0.
        let route = orthogonal_route(vec![pt(50.0, 10.0), pt(200.0, 10.0)]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 50.0, 50.0));
        let visible = vec!["A".into(), "B".into()];
        let input = RouteInput { visible_node_ids: &visible, node_rects: &node_rects };
        let rel = Relationship { from: "A", to: "B" };
        assert_eq!(non_endpoint_node_collision_count(&route, &rel, &input), 0);
    }

    // -----------------------------------------------------------------------
    // endpointStubRoute
    // -----------------------------------------------------------------------

    #[test]
    fn endpoint_stub_route_extends_short_start_stub() {
        // Node: shortRoute=[{0,25},{5,25},{5,50},{100,50}], from rect {0,0,50,50}
        // anchor (0,25) is on left side (x=rect.x=0). stub=5 < PORT_STUB=18.
        // After: adjacent→(-18,25), elbow x matches oldAdj(5)→-18.
        // Node confirmed: [{x:0,y:25},{x:-18,y:25},{x:-18,y:50},{x:100,y:50}]
        let route = orthogonal_route(vec![
            pt(0.0, 25.0), pt(5.0, 25.0), pt(5.0, 50.0), pt(100.0, 50.0),
        ]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(80.0, 30.0, 50.0, 50.0));
        let result = endpoint_stub_route(&route, "A", "B", &node_rects, 0);
        // After route_with_points simplification the points list may be reordered;
        // check the key values directly.
        assert_eq!(result.points[0], pt(0.0, 25.0), "anchor unchanged");
        assert_eq!(result.points[1], pt(-18.0, 25.0), "adjacent extended to PORT_STUB");
        assert_eq!(result.points[2].x, -18.0, "elbow x follows adjacent");
    }

    #[test]
    fn endpoint_stub_route_long_stub_unchanged() {
        // Node: endpointStubRoute with stub already >= PORT_STUB → route unchanged
        let route = orthogonal_route(vec![
            pt(0.0, 25.0), pt(-20.0, 25.0), pt(-20.0, 50.0), pt(100.0, 50.0),
        ]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(80.0, 30.0, 50.0, 50.0));
        let result = endpoint_stub_route(&route, "A", "B", &node_rects, 0);
        // stub length=20 >= 18 → unchanged
        assert_eq!(result.points[1], pt(-20.0, 25.0));
    }

    #[test]
    fn endpoint_stub_route_too_few_points_unchanged() {
        // Node: route with < 3 points → returned unchanged
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        let node_rects: IndexMap<String, Rect> = IndexMap::new();
        let result = endpoint_stub_route(&route, "A", "B", &node_rects, 0);
        assert_eq!(result.points, route.points);
    }

    // -----------------------------------------------------------------------
    // routeWithEndpointStubs
    // -----------------------------------------------------------------------

    #[test]
    fn route_with_endpoint_stubs_applies_both_ends() {
        // Route with short stubs at both ends on the same y; after extension both
        // stubs move outward but simplify_orthogonal_points collapses the now-
        // collinear run [{0,25},{-18,25},{195,25},{200,25}] → [{0,25},{200,25}].
        //
        // Node: routeWithEndpointStubs([{0,25},{5,25},{195,25},{200,25}], A, B)
        //   → after start-stub → routeWithPoints collapses → [{0,25},{200,25}]
        //   → then end-stub: 2 pts < 3 → unchanged → [{0,25},{200,25}]
        // Confirmed by node /tmp/test_c1_both_ends.js:
        //   Full result: [{"x":0,"y":25},{"x":200,"y":25}]
        let route = orthogonal_route(vec![
            pt(0.0, 25.0), pt(5.0, 25.0), pt(195.0, 25.0), pt(200.0, 25.0),
        ]);
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 50.0, 50.0));
        node_rects.insert("B".into(), rect(150.0, 0.0, 50.0, 50.0));
        let result = route_with_endpoint_stubs(&route, "A", "B", &node_rects);
        // Collinear simplification collapses to 2 points.
        assert_eq!(result.points.len(), 2);
        assert_eq!(result.points[0], pt(0.0, 25.0));
        assert_eq!(result.points[1], pt(200.0, 25.0));
    }

    // -----------------------------------------------------------------------
    // Pass C2 tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // axisAlignedRouteSegments
    // -----------------------------------------------------------------------

    #[test]
    fn axis_aligned_route_segments_l_shape() {
        // Node: route [{x:0,y:0},{x:100,y:0},{x:100,y:50}]
        // → [{ orientation:"horizontal", line:0, min:0, max:100, index:0 },
        //    { orientation:"vertical",   line:100, min:0, max:50, index:1 }]
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 50.0)]);
        let segs = axis_aligned_route_segments(&route);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].orientation, "horizontal");
        assert_eq!(segs[0].line, 0.0);
        assert_eq!(segs[0].min, 0.0);
        assert_eq!(segs[0].max, 100.0);
        assert_eq!(segs[0].index, 0);
        assert_eq!(segs[1].orientation, "vertical");
        assert_eq!(segs[1].line, 100.0);
        assert_eq!(segs[1].min, 0.0);
        assert_eq!(segs[1].max, 50.0);
        assert_eq!(segs[1].index, 1);
    }

    #[test]
    fn axis_aligned_route_segments_empty() {
        let route = orthogonal_route(vec![]);
        assert!(axis_aligned_route_segments(&route).is_empty());
    }

    // -----------------------------------------------------------------------
    // closeParallelSegmentPair
    // -----------------------------------------------------------------------

    #[test]
    fn close_parallel_segment_pair_found_close_parallel() {
        // Node: two horizontal routes at y=100 and y=105, spanning x=0..200
        // → close parallel (distance=5, overlap=200 >= 72)
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 105.0), pt(200.0, 105.0)]);
        let route_by_id = vec![("r1".into(), r1), ("r2".into(), r2)];
        let result = close_parallel_segment_pair(&route_by_id);
        assert!(result.is_some());
        let pair = result.unwrap();
        assert_eq!(pair.left_id, "r1");
        assert_eq!(pair.right_id, "r2");
        assert_eq!(pair.left.orientation, "horizontal");
        assert_eq!(pair.left.line, 100.0);
        assert_eq!(pair.right.line, 105.0);
    }

    #[test]
    fn close_parallel_segment_pair_exact_shared() {
        // Node: two routes on same horizontal line, overlapping > 1px
        // distance=0, overlap=100 > 1 → exactSharedSegment
        let r3 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r4 = orthogonal_route(vec![pt(50.0, 100.0), pt(150.0, 100.0)]);
        let route_by_id = vec![("r3".into(), r3), ("r4".into(), r4)];
        let result = close_parallel_segment_pair(&route_by_id);
        assert!(result.is_some());
    }

    #[test]
    fn close_parallel_segment_pair_not_found_too_far() {
        // Node: distance=11 > 10 → no close parallel
        let r5 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r6 = orthogonal_route(vec![pt(0.0, 111.0), pt(200.0, 111.0)]);
        let route_by_id = vec![("r5".into(), r5), ("r6".into(), r6)];
        assert!(close_parallel_segment_pair(&route_by_id).is_none());
    }

    #[test]
    fn close_parallel_segment_pair_not_found_short_overlap() {
        // Node: overlap=50 < 72 → no close parallel
        let r7 = orthogonal_route(vec![pt(0.0, 100.0), pt(50.0, 100.0)]);
        let r8 = orthogonal_route(vec![pt(0.0, 105.0), pt(50.0, 105.0)]);
        let route_by_id = vec![("r7".into(), r7), ("r8".into(), r8)];
        assert!(close_parallel_segment_pair(&route_by_id).is_none());
    }

    // -----------------------------------------------------------------------
    // shiftedInternalSegmentRoute
    // -----------------------------------------------------------------------

    #[test]
    fn shifted_internal_segment_route_middle_vertical() {
        // Node: Z-route [{x:0,y:50},{x:50,y:50},{x:50,y:100},{x:100,y:100}]
        // segment index=1 (vertical, x=50), delta=10 → shifts index 1,2 x by 10
        // → [{0,50},{60,50},{60,100},{100,100}]
        let route = orthogonal_route(vec![
            pt(0.0, 50.0), pt(50.0, 50.0), pt(50.0, 100.0), pt(100.0, 100.0),
        ]);
        let seg = RouteSegment { index: 1, orientation: "vertical", line: 50.0, min: 50.0, max: 100.0 };
        let result = shifted_internal_segment_route(&route, &seg, 10.0);
        assert!(result.is_some());
        let r = result.unwrap();
        // After simplification: points may be reordered but the middle x should shift
        // Node (js): [{x:0,y:50},{x:60,y:50},{x:60,y:100},{x:100,y:100}]
        assert_eq!(r.points.len(), 4);
        assert_eq!(r.points[1].x, 60.0);
        assert_eq!(r.points[2].x, 60.0);
    }

    #[test]
    fn shifted_internal_segment_route_endpoint_returns_none() {
        // Segment at index 0 (endpoint) → None
        let route = orthogonal_route(vec![pt(0.0, 50.0), pt(100.0, 50.0), pt(100.0, 100.0)]);
        let seg = RouteSegment { index: 0, orientation: "horizontal", line: 50.0, min: 0.0, max: 100.0 };
        assert!(shifted_internal_segment_route(&route, &seg, 10.0).is_none());
    }

    // -----------------------------------------------------------------------
    // routePairIndex
    // -----------------------------------------------------------------------

    #[test]
    fn route_pair_index_basic() {
        // Node (confirmed above):
        // rels=[{r1,A→B},{r2,B→A},{r3,A→B},{r4,C→D}]
        // routePairIndex(r1) = 0, (r2) = 1, (r3) = 2, (r4) = 0
        let rels: Vec<SeparationRelationship> = vec![
            SeparationRelationship { id: "r1".into(), from: "A".into(), to: "B".into() },
            SeparationRelationship { id: "r2".into(), from: "B".into(), to: "A".into() },
            SeparationRelationship { id: "r3".into(), from: "A".into(), to: "B".into() },
            SeparationRelationship { id: "r4".into(), from: "C".into(), to: "D".into() },
        ];
        assert_eq!(route_pair_index("r1", "A", "B", &rels), 0);
        assert_eq!(route_pair_index("r2", "B", "A", &rels), 1);
        assert_eq!(route_pair_index("r3", "A", "B", &rels), 2);
        assert_eq!(route_pair_index("r4", "C", "D", &rels), 0);
    }

    // -----------------------------------------------------------------------
    // routeEndpointsArePerpendicular
    // -----------------------------------------------------------------------

    #[test]
    fn route_endpoints_are_perpendicular_true() {
        // Node: A at (0,0,100,50), B at (200,0,100,50)
        // Route exits A's right side (x=100) horizontally toward B's left side (x=200).
        // Node: routeEndpointsArePerpendicular(route, rel, input) = true
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 100.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 100.0, 50.0));
        let route = orthogonal_route(vec![pt(100.0, 25.0), pt(150.0, 25.0), pt(200.0, 25.0)]);
        assert!(route_endpoints_are_perpendicular(&route, "A", "B", &node_rects));
    }

    #[test]
    fn route_endpoints_are_perpendicular_false() {
        // Node: exit not parallel to side → false
        let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
        node_rects.insert("A".into(), rect(0.0, 0.0, 100.0, 50.0));
        node_rects.insert("B".into(), rect(200.0, 0.0, 100.0, 50.0));
        // Exits right of A (y=25) but adjacent is at y=50 → not perpendicular
        let route = orthogonal_route(vec![pt(100.0, 25.0), pt(150.0, 50.0), pt(200.0, 25.0)]);
        // After simplification this might be 3 points but the perpendicular check still fails.
        // Point 0 is on right side of A (x=100), adjacent point[1].y must equal point[0].y.
        // point[1].y=50 != point[0].y=25 → false.
        assert!(!route_endpoints_are_perpendicular(&route, "A", "B", &node_rects));
    }

    // -----------------------------------------------------------------------
    // closeParallelRunCountForRoutes
    // -----------------------------------------------------------------------

    #[test]
    fn close_parallel_run_count_for_routes_two_routes() {
        // Node: 2 close parallel horizontal routes (distance=5, overlap=200)
        // → count = 1
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 105.0), pt(200.0, 105.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        assert_eq!(close_parallel_run_count_for_routes(&rbd), 1);
    }

    #[test]
    fn close_parallel_run_count_for_routes_none() {
        // Node: distance=11 → 0
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 111.0), pt(200.0, 111.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        assert_eq!(close_parallel_run_count_for_routes(&rbd), 0);
    }

    // -----------------------------------------------------------------------
    // closeParallelRunCountBetween
    // -----------------------------------------------------------------------

    #[test]
    fn close_parallel_run_count_between_pair() {
        // Node: same as above but testing the between version
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 105.0), pt(200.0, 105.0)]);
        assert_eq!(close_parallel_run_count_between(&r1, &r2), 1);
    }

    #[test]
    fn close_parallel_run_count_between_none() {
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 111.0), pt(200.0, 111.0)]);
        assert_eq!(close_parallel_run_count_between(&r1, &r2), 0);
    }

    // -----------------------------------------------------------------------
    // crossingPairKey
    // -----------------------------------------------------------------------

    #[test]
    fn crossing_pair_key_canonical() {
        // Node: crossingPairKey(0,1)="0:1", crossingPairKey(1,0)="0:1"
        assert_eq!(crossing_pair_key(0, 1), "0:1");
        assert_eq!(crossing_pair_key(1, 0), "0:1");
        assert_eq!(crossing_pair_key(2, 3), "2:3");
        assert_eq!(crossing_pair_key(3, 2), "2:3");
    }

    // -----------------------------------------------------------------------
    // routeSetStats
    // -----------------------------------------------------------------------

    #[test]
    fn route_set_stats_shared_segment() {
        // Node: two routes sharing horizontal segment at y=100, overlap=100 > 1
        // → {repeatedCrossings:0, sharedSegments:1}
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(50.0, 100.0), pt(150.0, 100.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        let stats = route_set_stats(&rbd);
        assert_eq!(stats.shared_segments, 1);
        assert_eq!(stats.repeated_crossings, 0);
    }

    #[test]
    fn route_set_stats_no_overlap() {
        // Node: different y lines → {repeatedCrossings:0, sharedSegments:0}
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 200.0), pt(200.0, 200.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        let stats = route_set_stats(&rbd);
        assert_eq!(stats.shared_segments, 0);
        assert_eq!(stats.repeated_crossings, 0);
    }

    // -----------------------------------------------------------------------
    // routeSeparationScore / isBetterRouteSeparation
    // -----------------------------------------------------------------------

    #[test]
    fn route_separation_score_order() {
        // Node (confirmed above):
        // score({nextCloseCount:1,sharedSeg:0,repCross:0,pairClose:0,bends:2,dist:5})
        //   = [1,0,0,0,2,5]
        let stats = RouteSetStats { repeated_crossings: 0, shared_segments: 0 };
        let s1 = route_separation_score(1, &stats, 0, 2, 5.0);
        assert_eq!(s1, [1.0, 0.0, 0.0, 0.0, 2.0, 5.0]);
        let s2 = route_separation_score(0, &stats, 0, 3, 5.0);
        assert_eq!(s2, [0.0, 0.0, 0.0, 0.0, 3.0, 5.0]);
        // s2 beats s1 (lower closeCount)
        assert!(is_better_route_separation(&s2, Some(&s1)));
        assert!(!is_better_route_separation(&s1, Some(&s2)));
    }

    #[test]
    fn is_better_route_separation_vs_none() {
        // Node: any score beats None
        let stats = RouteSetStats { repeated_crossings: 0, shared_segments: 0 };
        let s = route_separation_score(0, &stats, 0, 0, 5.0);
        assert!(is_better_route_separation(&s, None));
    }

    #[test]
    fn is_better_route_separation_distance_tiebreak() {
        // Node: distance=7 score=[0,0,0,0,2,7] vs distance=5 score=[0,0,0,0,2,5]
        // → dist5 wins (smaller abs distance)
        let stats = RouteSetStats { repeated_crossings: 0, shared_segments: 0 };
        let s_dist7 = route_separation_score(0, &stats, 0, 2, 7.0);
        let s_dist5 = route_separation_score(0, &stats, 0, 2, 5.0);
        assert!(!is_better_route_separation(&s_dist7, Some(&s_dist5)));
        assert!(is_better_route_separation(&s_dist5, Some(&s_dist7)));
    }

    // -----------------------------------------------------------------------
    // routeSetScore / isBetterRouteSet
    // -----------------------------------------------------------------------

    #[test]
    fn route_set_score_basic() {
        // Node: two non-parallel routes → [0,0,0,totalBends]
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 200.0), pt(200.0, 200.0)]);
        let rbd = vec![("r1".into(), r1), ("r2".into(), r2)];
        let score = route_set_score(&rbd);
        assert_eq!(score[0], 0.0); // no close parallel
        assert_eq!(score[1], 0.0); // no shared segments
        assert_eq!(score[2], 0.0); // no repeated crossings
        assert_eq!(score[3], 0.0); // 0+0 bends
    }

    #[test]
    fn is_better_route_set_vs_none() {
        let score: [f64; 4] = [0.0, 0.0, 0.0, 2.0];
        assert!(is_better_route_set(&score, None));
    }

    #[test]
    fn is_better_route_set_lower_wins() {
        let s1: [f64; 4] = [1.0, 0.0, 0.0, 0.0];
        let s2: [f64; 4] = [0.0, 0.0, 0.0, 0.0];
        assert!(is_better_route_set(&s2, Some(&s1)));
        assert!(!is_better_route_set(&s1, Some(&s2)));
    }

    // -----------------------------------------------------------------------
    // separateCloseParallelRoutes — small fixture
    // -----------------------------------------------------------------------

    #[test]
    fn separate_close_parallel_routes_no_close_pair_unchanged() {
        // Node: routes far apart → separateCloseParallelRoutes returns them unchanged
        // Two routes at y=100 and y=200 (100px apart) with no closeness → immediately break.
        let r1 = orthogonal_route(vec![pt(0.0, 100.0), pt(200.0, 100.0)]);
        let r2 = orthogonal_route(vec![pt(0.0, 200.0), pt(200.0, 200.0)]);
        let planned = vec![("r1".into(), r1.clone()), ("r2".into(), r2.clone())];
        let rels: Vec<SeparationRelationship> = vec![
            SeparationRelationship { id: "r1".into(), from: "A".into(), to: "B".into() },
            SeparationRelationship { id: "r2".into(), from: "C".into(), to: "D".into() },
        ];
        let node_rects: IndexMap<String, Rect> = IndexMap::new();
        let fixed_ports: IndexMap<String, bool> = IndexMap::new();
        let result = separate_close_parallel_routes(
            &planned, &rels, &node_rects, &fixed_ports, &NoopReroute,
        );
        // No close pair found → same routes returned.
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "r1");
        assert_eq!(result[1].0, "r2");
    }

    // -----------------------------------------------------------------------
    // alternateMiddleDoglegRoutes
    // -----------------------------------------------------------------------

    #[test]
    fn alternate_middle_dogleg_horizontal() {
        // Node (confirmed above):
        // route [{x:0,y:50},{x:82,y:50},{x:120,y:50},{x:120,y:80},{x:200,y:80}]
        // → horizontal dogleg → 2 alternatives
        // alt0 points: [{0,50},{82,50},{82,80},{120,80},{200,80}]
        // alt1 points: [{0,50},{82,50},{82,65},{120,65},{120,80},{200,80}]
        let route = orthogonal_route(vec![
            pt(0.0, 50.0), pt(82.0, 50.0), pt(120.0, 50.0), pt(120.0, 80.0), pt(200.0, 80.0),
        ]);
        let alts = alternate_middle_dogleg_routes(&route);
        assert_eq!(alts.len(), 2);
        // alt0: 5 points (sourceStub.x, targetStub.y as new middle)
        // After route_with_points simplification the 5-point route may simplify
        // if any consecutive points are collinear, but let's check the shape.
        // The new middle is {x:82,y:80} which connects {82,50}→{82,80}→{120,80} — that's
        // a valid L shape, so no simplification removes points.
        let alt0_pts = &alts[0].points;
        // {0,50}→{82,50}→{82,80}→{120,80}→{200,80}
        assert_eq!(alt0_pts[0], pt(0.0, 50.0));
        assert_eq!(alt0_pts[1], pt(82.0, 50.0));
        assert_eq!(alt0_pts[2], pt(82.0, 80.0));
        assert_eq!(alt0_pts[3], pt(120.0, 80.0));
        assert_eq!(alt0_pts[4], pt(200.0, 80.0));
        // alt1: 6 points with gutter_y=(50+80)/2=65
        let alt1_pts = &alts[1].points;
        assert_eq!(alt1_pts[0], pt(0.0, 50.0));
        assert_eq!(alt1_pts[1], pt(82.0, 50.0));
        assert_eq!(alt1_pts[2], pt(82.0, 65.0));
        assert_eq!(alt1_pts[3], pt(120.0, 65.0));
        assert_eq!(alt1_pts[4], pt(120.0, 80.0));
        assert_eq!(alt1_pts[5], pt(200.0, 80.0));
    }

    #[test]
    fn alternate_middle_dogleg_wrong_length_returns_empty() {
        // Node: route with != 5 points → []
        let route = orthogonal_route(vec![pt(0.0, 0.0), pt(100.0, 0.0)]);
        assert!(alternate_middle_dogleg_routes(&route).is_empty());
    }

    #[test]
    fn alternate_middle_dogleg_vertical() {
        // Node: vertical dogleg
        // route [{x:50,y:0},{x:50,y:82},{x:100,y:82},{x:100,y:200},{x:80,y:200}]
        // → verticalEndpointDogleg: start.x(50)==sourceStub.x(50)==middleA.x(100)? NO
        // Let's construct a real vertical dogleg:
        // start.x == sourceStub.x == middleA.x AND middleA.y == targetStub.y AND targetStub.x == end.x
        // route [{x:50,y:0},{x:50,y:82},{x:50,y:120},{x:100,y:120},{x:100,y:200}]
        // start.x(50)==sourceStub.x(50)==middleA.x(50): ✓
        // middleA.y(120)==targetStub.y(120): ✓
        // targetStub.x(100)==end.x(100): ✓
        // sourceStub.y(82) != targetStub.y(120): ✓ (needed for gutterX)
        let route = orthogonal_route(vec![
            pt(50.0, 0.0), pt(50.0, 82.0), pt(50.0, 120.0), pt(100.0, 120.0), pt(100.0, 200.0),
        ]);
        let alts = alternate_middle_dogleg_routes(&route);
        assert_eq!(alts.len(), 2);
        // gutter_x = (50+100)/2 = 75
        let alt1_pts = &alts[1].points;
        assert_eq!(alt1_pts[2], pt(75.0, 82.0)); // {gutterX, sourceStub.y}
        assert_eq!(alt1_pts[3], pt(75.0, 120.0)); // {gutterX, targetStub.y}
    }

    // -----------------------------------------------------------------------
    // spreadUnitSlots — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn spread_unit_slots_zero_half_widths_reduces_to_even_spread() {
        // Node: spreadUnitSlots([0,0,0], 100) = [-25, 0, 25]
        let slots = spread_unit_slots(&[0.0, 0.0, 0.0], 100.0);
        assert_eq!(slots.len(), 3);
        assert!((slots[0] - -25.0).abs() < 1e-9);
        assert!((slots[1] - 0.0).abs() < 1e-9);
        assert!((slots[2] - 25.0).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_single_unit_zero_width_centres() {
        // Node: spreadUnitSlots([0], 100) = [0]  (single unit → centre = 0)
        // With hw=0: content=0, slack=100, gap=100/2=50, cursor=-50+50+0=0
        let slots = spread_unit_slots(&[0.0], 100.0);
        assert_eq!(slots.len(), 1);
        assert!((slots[0] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_nonzero_half_widths() {
        // Node: spreadUnitSlots([6,6,6], 54) = [-16.5, 0, 16.5]
        let slots = spread_unit_slots(&[6.0, 6.0, 6.0], 54.0);
        assert_eq!(slots.len(), 3);
        assert!((slots[0] - -16.5).abs() < 1e-9);
        assert!((slots[1] - 0.0).abs() < 1e-9);
        assert!((slots[2] - 16.5).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_no_slack_falls_back_to_even_centres() {
        // Node: spreadUnitSlots([6,6], 20) → slack=20-24=-4 ≤ 0 → fallback
        // fallback: [1/(2+1)-0.5, 2/(2+1)-0.5] * 20 = [-3.333..., 3.333...]
        let slots = spread_unit_slots(&[6.0, 6.0], 20.0);
        assert_eq!(slots.len(), 2);
        let expected0 = (1.0_f64 / 3.0 - 0.5) * 20.0;
        let expected1 = (2.0_f64 / 3.0 - 0.5) * 20.0;
        assert!((slots[0] - expected0).abs() < 1e-9);
        assert!((slots[1] - expected1).abs() < 1e-9);
    }

    #[test]
    fn spread_unit_slots_two_units_with_widths() {
        // Node: spreadUnitSlots([5,5], 100) = [-18.333..., 18.333...]
        let slots = spread_unit_slots(&[5.0, 5.0], 100.0);
        assert_eq!(slots.len(), 2);
        assert!((slots[0] - -18.333_333_333_333_332).abs() < 1e-9);
        assert!((slots[1] - 18.333_333_333_333_336).abs() < 1e-9);
    }

    // -----------------------------------------------------------------------
    // recenteredEndpointRouteWithAnchors
    // -----------------------------------------------------------------------

    #[test]
    fn recentered_endpoint_route_with_anchors_uses_side_anchor() {
        // TDD: decision-diamond node has sideAnchors.right = {x:484.87, y:607}.
        // The rect geometric right edge midpoint is at y = 588 + 38/2 = 607 (same y here,
        // but different x: rect.x+rect.width = 477 ≠ 484.87).
        // Route starts at the geometric right midpoint (x=477, y=607).
        // After recenteredEndpointRouteWithAnchors the start must move to the sideAnchor (x=484.87).
        use crate::route_ports::SideAnchors;

        let r = Rect { x: 439.0, y: 588.0, width: 38.0, height: 38.0 };
        // A multi-point route starting at geometric right midpoint
        let route = orthogonal_route(vec![
            pt(477.0, 607.0),   // geometric right midpoint (rect.x+rect.width, rect.y+rect.height/2)
            pt(495.0, 607.0),   // port stub
            pt(792.0, 607.0),   // horizontal run
            pt(792.0, 300.0),   // vertical run
        ]);
        let anchors = SideAnchors {
            right: Some(pt(484.87, 607.0)),
            ..Default::default()
        };

        let without = recentered_endpoint_route(&route, 0, &r, "right");
        let with_anchors = recentered_endpoint_route_with_anchors(&route, 0, &r, "right", Some(&anchors));

        // Without anchors: endpoint stays at geometric midpoint x (no sideAnchors)
        // The rect.x+rect.width = 477, so endpointSide sees x=477 as "right" → anchor y stays
        // (the exact move depends on geometry, but endpoint must still be on right edge)
        // With anchors: endpoint moves to sideAnchor x=484.87 (the diamond tip)
        assert_eq!(
            with_anchors.points[0].x,
            484.87,
            "sideAnchor x used instead of geometric right edge"
        );
        assert_eq!(
            with_anchors.points[0].y,
            607.0,
            "sideAnchor y preserved"
        );
        // Without anchors the start differs (uses geometric right x=477)
        assert_ne!(
            without.points[0].x,
            with_anchors.points[0].x,
            "without anchors gives different x than with anchors"
        );
    }

    #[test]
    fn recentered_endpoint_route_with_anchors_none_matches_plain() {
        // When side_anchors is None, result must be identical to recentered_endpoint_route.
        // Node: confirms no regression on non-diamond nodes.
        let r = Rect { x: 100.0, y: 200.0, width: 60.0, height: 80.0 };
        let route = orthogonal_route(vec![
            pt(100.0, 240.0),
            pt(82.0, 240.0),
            pt(82.0, 300.0),
            pt(200.0, 300.0),
        ]);
        let plain = recentered_endpoint_route(&route, 0, &r, "left");
        let with_none = recentered_endpoint_route_with_anchors(&route, 0, &r, "left", None);
        assert_eq!(plain.points, with_none.points, "None anchors must match plain recentered route");
        assert_eq!(plain.d, with_none.d);
    }
}
