//! Faithful port of `viewer/src/routing/routeEdges.js`.
//!
//! Split into cohesive submodules; all public items are re-exported here so
//! external code using `crate::route_edges::X` paths keeps working unchanged.
//!
//! Submodule responsibilities:
//! - `types` — `RouteData`, `RouteInput`, `Relationship`, `AxisAlignedSegment`.
//! - `helpers` — geometry helpers, collision checks, route-point construction, offset/polyline utilities, shared-segment rendering.
//! - `crossings` — route-pair crossing/gutter analysis.
//! - `inputs` — engine-free routing input types (`RouteEdgesInput`, `InputRelationship`, `NodeRect`, `CorpusPlanStats`).

pub mod types;
pub mod helpers;
pub mod crossings;
pub mod inputs;

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

// -- crossings --
pub use crossings::{crossings_between, crossings_involving, gutter_lane_of};

// -- inputs (engine-free routing input types) --
pub use inputs::{CorpusPlanStats, InputRelationship, NodeRect, RouteEdgesInput};

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




    // -----------------------------------------------------------------------
    // nonEndpointNodeCollisionCount
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // endpointStubRoute
    // -----------------------------------------------------------------------




    // -----------------------------------------------------------------------
    // routeWithEndpointStubs
    // -----------------------------------------------------------------------


    // -----------------------------------------------------------------------
    // Pass C2 tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // axisAlignedRouteSegments
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // closeParallelSegmentPair
    // -----------------------------------------------------------------------





    // -----------------------------------------------------------------------
    // shiftedInternalSegmentRoute
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // routePairIndex
    // -----------------------------------------------------------------------


    // -----------------------------------------------------------------------
    // routeEndpointsArePerpendicular
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // closeParallelRunCountForRoutes
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // closeParallelRunCountBetween
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // crossingPairKey
    // -----------------------------------------------------------------------


    // -----------------------------------------------------------------------
    // routeSetStats
    // -----------------------------------------------------------------------



    // -----------------------------------------------------------------------
    // routeSeparationScore / isBetterRouteSeparation
    // -----------------------------------------------------------------------




    // -----------------------------------------------------------------------
    // routeSetScore / isBetterRouteSet
    // -----------------------------------------------------------------------




    // -----------------------------------------------------------------------
    // separateCloseParallelRoutes — small fixture
    // -----------------------------------------------------------------------


    // -----------------------------------------------------------------------
    // alternateMiddleDoglegRoutes
    // -----------------------------------------------------------------------




    // -----------------------------------------------------------------------
    // spreadUnitSlots — Node goldens
    // -----------------------------------------------------------------------






    // -----------------------------------------------------------------------
    // recenteredEndpointRouteWithAnchors
    // -----------------------------------------------------------------------


}
