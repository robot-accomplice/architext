//! Faithful port of `viewer/src/routing/routeMountModel.js` (1005 loc).
//!
//! Pass B of the Phase 1B routeEdges rewrite. Every exported function the
//! orchestration layer calls is reproduced here. Functions that depend on
//! `buildRouteForSides` (a JS callback) accept an `Option<&dyn BuildRouteForSides>`
//! trait object — callers that do not yet have that wired pass `None` and the
//! functions no-op exactly as the JS `if (!buildRouteForSides) return` guards do.
//!
//! Decomposed into submodules:
//! - `types`        — input/output types and the `BuildRouteForSides` trait
//! - `helpers`      — private utility functions (pub(super))
//! - `cost`         — cost functions and mount cost computation
//! - `relief`       — crowded-surface relief passes
//! - `optimize`     — facing-alignment and reciprocal-parallel optimization
//! - `distribution` — even-distribution passes across node faces

pub mod cost;
pub mod distribution;
pub mod helpers;
pub mod optimize;
pub mod reciprocal;
pub mod relief;
pub mod types;

// ---------------------------------------------------------------------------
// Re-export public API (preserves external `crate::route_mount_model::X` paths)
// ---------------------------------------------------------------------------

// types
pub use types::{
    BuildRouteForSides, GutterBridge, MountCostFactors, MountInput, MountRect, MountRelationship,
    MountTarget, ReliefResult, SurfaceInfo,
};

// cost
pub use cost::{
    apply_offset_with_match, dogleg_count, excess_length, intent_mismatch_count,
    mount_assignment_cost, mount_cost_factors, route_intersections, strict_crossing_count,
    surface_cramped_units, surface_spacing_cost, surfaces_of,
};

// relief
pub use relief::{build_monotonic_staircase, build_reciprocal_gutter_bridge, relieve_crowded_surfaces};

// optimize
pub use optimize::{
    center_solo_reciprocal_pair_surfaces, optimize_mount_assignments, realign_facing_endpoints,
    reciprocal_parallel_moves, route_unjustified_non_facing,
};

// reciprocal
pub use reciprocal::{
    order_gutter_lanes_by_target, recenter_singleton_side_endpoints,
    reduce_crossings_by_surface_swaps, reorder_shared_surface_mounts,
    route_reciprocal_pairs_parallel, spread_shared_side_endpoints,
};

// distribution
pub use distribution::{
    crossing_count_involving, distribute_facing_reciprocal_surfaces,
    distribute_surface_mount_units, keep_mount_moves_unless_worse,
    mirror_self_crossing_bundles, shared_segment_count_involving, straighten_self_crossing_pairs,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_constants::BRIDGE_GUTTER_CLEARANCE;
    use crate::route_edges::RouteData;
    use crate::model::{Point, Rect};
    use indexmap::IndexMap;

    /// Build a minimal orthogonal route from a point list.
    fn mk_route(points: Vec<Point>) -> RouteData {
        use crate::route_geometry::bounds_for_points;
        let all: Vec<Point> = points.clone();
        let sb = bounds_for_points(&all);
        RouteData {
            d: String::new(),
            points,
            controls: None,
            samples: vec![],
            sample_bounds: sb,
            bends: 0,
            label_x: 0.0,
            label_y: 0.0,
            style: "orthogonal".to_string(),
            extra: indexmap::IndexMap::new(),
        }
    }

    fn mk_rect(x: f64, y: f64, w: f64, h: f64) -> MountRect {
        MountRect { rect: Rect { x, y, width: w, height: h }, fixed_ports: false, side_anchors: None }
    }

    fn mk_rel(id: &str, from: &str, to: &str) -> MountRelationship {
        MountRelationship {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            relationship_type: "flow".to_string(),
            preferred_start_side: None,
            preferred_end_side: None,
            display_index: 0,
            kind: None,
            return_of: None,
            outcome: None,
            step_id: None,
            flow_id: None,
        }
    }

    // -----------------------------------------------------------------------
    // surfaceCrampedUnits — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn cramped_units_positions_within_gap() {
        // [10, 14] in length 40: guards=[0,10,14,40], gaps=[10,4,26]
        // gap=4 == MIN_LEGIBLE_GAP (4.0), NOT < 4 → no shortfall
        // Node: surfaceCrampedUnits([10, 14], 40) = 0
        assert_eq!(surface_cramped_units(&[10.0, 14.0], 40.0), 0.0);
    }

    #[test]
    fn cramped_units_empty_positions() {
        // Node: surfaceCrampedUnits([], 40) = 0
        assert_eq!(surface_cramped_units(&[], 40.0), 0.0);
    }

    #[test]
    fn cramped_units_single_position() {
        // Node: surfaceCrampedUnits([10], 40) = 0
        assert_eq!(surface_cramped_units(&[10.0], 40.0), 0.0);
    }

    #[test]
    fn cramped_units_three_tight_positions() {
        // [5, 7, 10] in length 20: guards=[0,5,7,10,20]
        // gaps = [5, 2, 3, 10] — gap 2 < 4 (shortfall 2), gap 3 < 4 (shortfall 1)
        // Node: surfaceCrampedUnits([5, 7, 10], 20) = 3
        assert_eq!(surface_cramped_units(&[5.0, 7.0, 10.0], 20.0), 3.0);
    }

    // -----------------------------------------------------------------------
    // excessLength — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn excess_length_straight_edge() {
        // Route [{x:0,y:0},{x:100,y:0}], from rect 0..10 to rect 90..100
        // nodeGapLength = 90 - 10 = 80; routeLength = 100; excess = 20
        // Node: excessLength = 20
        let route = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let from_rect = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 0.0, width: 10.0, height: 10.0 };
        assert_eq!(excess_length(&route, Some(&from_rect), Some(&to_rect)), 20.0);
    }

    #[test]
    fn excess_length_detour() {
        // L-shaped route from (0,0)→(50,0)→(50,50)→(100,50): length=150
        // fromRect 0,0,10,10 toRect 90,40,10,10: gapX=80, gapY=30, gap=110
        // Node: excessLength = 40
        let route = mk_route(vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 50.0, y: 0.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 100.0, y: 50.0 },
        ]);
        let from_rect = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 40.0, width: 10.0, height: 10.0 };
        assert_eq!(excess_length(&route, Some(&from_rect), Some(&to_rect)), 40.0);
    }

    // -----------------------------------------------------------------------
    // doglegCount — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn dogleg_count_straight_no_dogleg() {
        // Node: doglegCount(straight, A, B) = 0
        let route = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let from_rect = Rect { x: 0.0, y: 20.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 20.0, width: 10.0, height: 10.0 };
        assert_eq!(dogleg_count(&route, Some(&from_rect), Some(&to_rect)), 0.0);
    }

    #[test]
    fn dogleg_count_backtrack() {
        // [{x:0,y:0},{x:60,y:0},{x:40,y:0},{x:100,y:0}]: goes right then back left then right
        // x_dir=1 (to right of from). The segment 60→40 has dx<0 = -x_dir → count 1
        // Node: doglegCount = 1
        let route = mk_route(vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 60.0, y: 0.0 },
            Point { x: 40.0, y: 0.0 },
            Point { x: 100.0, y: 0.0 },
        ]);
        let from_rect = Rect { x: 0.0, y: 20.0, width: 10.0, height: 10.0 };
        let to_rect = Rect { x: 90.0, y: 20.0, width: 10.0, height: 10.0 };
        assert_eq!(dogleg_count(&route, Some(&from_rect), Some(&to_rect)), 1.0);
    }

    // -----------------------------------------------------------------------
    // strictCrossingCount / routeIntersections — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn strict_crossing_counts_interior_x() {
        // H: (0,50)-(100,50); V: (50,0)-(50,100) — strictly straddle each other
        // Node: strictCrossingCount = 1
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let v = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 100.0 }]);
        assert_eq!(strict_crossing_count(&h, &v), 1.0);
    }

    #[test]
    fn strict_crossing_misses_t_junction() {
        // H: (0,50)-(100,50); V-T: (50,0)-(50,50) — touches endpoint, not interior
        // Node: strictCrossingCount = 0
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let vt = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 50.0 }]);
        assert_eq!(strict_crossing_count(&h, &vt), 0.0);
    }

    #[test]
    fn route_intersections_counts_t_junction() {
        // Node: routeIntersections for T-junction = 1
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let vt = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 50.0 }]);
        assert_eq!(route_intersections(&h, &vt), 1);
    }

    #[test]
    fn route_intersections_crossing() {
        // Node: routeIntersections for clean X = 1
        let h = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let v = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 100.0 }]);
        assert_eq!(route_intersections(&h, &v), 1);
    }

    // -----------------------------------------------------------------------
    // mountCostFactors — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn mount_cost_factors_crossing_diagram() {
        // Two routes that strictly cross; from Node run:
        // factors.crossing = 1, factors.intentMismatch = 4, factors.length = 40
        // cost = 3000*1 + 1500*4 + 6*40 = 3000+6000+240 = 9240
        let mut route_by_id: IndexMap<String, RouteData> = IndexMap::new();
        route_by_id.insert("e1".to_string(), mk_route(vec![
            Point { x: 0.0, y: 50.0 }, Point { x: 200.0, y: 50.0 }
        ]));
        route_by_id.insert("e2".to_string(), mk_route(vec![
            Point { x: 100.0, y: 0.0 }, Point { x: 100.0, y: 100.0 }
        ]));

        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("e1".to_string(), mk_rel("e1", "A", "B"));
        rel_by_id.insert("e2".to_string(), mk_rel("e2", "C", "D"));

        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("A".to_string(), mk_rect(0.0, 45.0, 10.0, 10.0));
        node_rects.insert("B".to_string(), mk_rect(190.0, 45.0, 10.0, 10.0));
        node_rects.insert("C".to_string(), mk_rect(95.0, 0.0, 10.0, 10.0));
        node_rects.insert("D".to_string(), mk_rect(95.0, 90.0, 10.0, 10.0));

        let visible = vec!["A".to_string(), "B".to_string(), "C".to_string(), "D".to_string()];
        let lane_idx: IndexMap<String, i64> = IndexMap::new();
        let row_idx: IndexMap<String, i64> = IndexMap::new();
        let input = MountInput {
            visible_node_ids: &visible,
            node_rects: &node_rects,
            lane_index_by_node: &lane_idx,
            row_index_by_node: &row_idx,
            canvas_width: 400.0,
            canvas_height: 200.0,
        };

        let factors = mount_cost_factors(&route_by_id, &rel_by_id, &input);
        assert_eq!(factors.crossing, 1.0, "crossing");
        assert_eq!(factors.intent_mismatch, 4.0, "intentMismatch");
        assert_eq!(factors.length, 40.0, "length");
        assert_eq!(factors.collision, 0.0, "collision");
        assert_eq!(factors.shared_segment, 0.0, "sharedSegment");

        let cost = mount_assignment_cost(&route_by_id, &rel_by_id, &input);
        assert_eq!(cost, 9240.0);
    }

    // -----------------------------------------------------------------------
    // surfacesOf — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn surfaces_of_single_horizontal_route() {
        // e1: A(left side 0,45,10,10) → B(right side 90,45,10,10)
        // route (0,50)→(100,50): point 0 is on A.left, point-1 on B.right
        // A.left: axisStart = rect.y = 45; pos = point.y - axisStart = 50-45 = 5
        // B.right: axisStart = rect.y = 45; pos = 50-45 = 5
        // Node: surfaces keys = ["A left","B right"], positions both [5]
        let mut route_by_id: IndexMap<String, RouteData> = IndexMap::new();
        route_by_id.insert("e1".to_string(), mk_route(vec![
            Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }
        ]));
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("e1".to_string(), mk_rel("e1", "A", "B"));
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("A".to_string(), mk_rect(0.0, 45.0, 10.0, 10.0));
        node_rects.insert("B".to_string(), mk_rect(90.0, 45.0, 10.0, 10.0));

        let surfs = surfaces_of(&route_by_id, &rel_by_id, &node_rects);
        assert!(surfs.contains_key("A left"), "A left key missing");
        assert!(surfs.contains_key("B right"), "B right key missing");
        assert_eq!(surfs["A left"].positions, vec![5.0]);
        assert_eq!(surfs["B right"].positions, vec![5.0]);
    }

    // -----------------------------------------------------------------------
    // buildMonotonicStaircase — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn monotonic_staircase_left_right_keeps_points_when_same_y() {
        // start=right, end=left, p_a.y == p_b.y → [pA, pB]
        let route = mk_route(vec![
            Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }
        ]);
        let result = build_monotonic_staircase(&route, "right", "left", 50.0);
        assert_eq!(result.points.len(), 2);
        assert_eq!(result.points[0].x, 0.0);
        assert_eq!(result.points[1].x, 100.0);
    }

    #[test]
    fn monotonic_staircase_right_left_different_y() {
        // Node: staircase route from (0,50)→(50,50)→(50,100)→(100,100), startSide=right, endSide=left, elbow=50
        // horiz both → pA.y(50) != pB.y(100) → [pA, {x:elbow,y:pA.y}, {x:elbow,y:pB.y}, pB]
        // = [{x:0,y:50},{x:50,y:50},{x:50,y:100},{x:100,y:100}]
        // Node confirms staircase points = same as input since they already form a staircase
        let route = mk_route(vec![
            Point { x: 0.0, y: 50.0 },
            Point { x: 50.0, y: 50.0 },
            Point { x: 50.0, y: 100.0 },
            Point { x: 100.0, y: 100.0 },
        ]);
        let result = build_monotonic_staircase(&route, "right", "left", 50.0);
        // pA = first point = (0,50), pB = last = (100,100)
        // horiz(right) && horiz(left), pA.y=50 != pB.y=100
        // points = [(0,50),(50,50),(50,100),(100,100)]
        assert_eq!(result.points[0], Point { x: 0.0, y: 50.0 });
        assert_eq!(result.points[1], Point { x: 50.0, y: 50.0 });
        assert_eq!(result.points[2], Point { x: 50.0, y: 100.0 });
        assert_eq!(result.points[3], Point { x: 100.0, y: 100.0 });
    }

    // -----------------------------------------------------------------------
    // buildReciprocalGutterBridge — Node goldens
    // -----------------------------------------------------------------------

    #[test]
    fn reciprocal_gutter_bridge_top() {
        // Node: bridge.request.points = [{x:12,y:40},{x:12,y:26},{x:88,y:26},{x:88,y:40}]
        //       bridge.return.points  = [{x:92,y:40},{x:92,y:12},{x:8,y:12},{x:8,y:40}]
        let req_rel = mk_rel("e1", "A", "B");
        let ret_rel = mk_rel("e2", "B", "A");
        let req_route = mk_route(vec![Point { x: 5.0, y: 50.0 }, Point { x: 95.0, y: 50.0 }]);
        let ret_route = mk_route(vec![Point { x: 95.0, y: 50.0 }, Point { x: 5.0, y: 50.0 }]);
        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("A".to_string(), mk_rect(0.0, 40.0, 20.0, 20.0));
        node_rects.insert("B".to_string(), mk_rect(80.0, 40.0, 20.0, 20.0));

        let bridge = build_reciprocal_gutter_bridge(
            &req_rel, &ret_rel, &req_route, &ret_route,
            &node_rects, "top", BRIDGE_GUTTER_CLEARANCE,
        ).expect("bridge should succeed");

        // request goes top side: surfYa = ra.y = 40, surfYb = rb.y = 40
        // edge = min(40,40) - 14 = 26, laneReq=26, laneRet=26-14=12
        assert_eq!(bridge.request.points[0], Point { x: 12.0, y: 40.0 });
        assert_eq!(bridge.request.points[1], Point { x: 12.0, y: 26.0 });
        assert_eq!(bridge.request.points[2], Point { x: 88.0, y: 26.0 });
        assert_eq!(bridge.request.points[3], Point { x: 88.0, y: 40.0 });
        assert_eq!(bridge.ret.points[0], Point { x: 92.0, y: 40.0 });
        assert_eq!(bridge.ret.points[1], Point { x: 92.0, y: 12.0 });
        assert_eq!(bridge.ret.points[2], Point { x: 8.0, y: 12.0 });
        assert_eq!(bridge.ret.points[3], Point { x: 8.0, y: 40.0 });
    }

    // -----------------------------------------------------------------------
    // surfaceSpacingCost
    // -----------------------------------------------------------------------

    #[test]
    fn surface_spacing_cost_zero_when_no_cramping() {
        // [10, 20] in length 40: all gaps >= MIN_LEGIBLE_GAP=4 → cost = 0
        assert_eq!(surface_spacing_cost(&[10.0, 20.0], 40.0), 0.0);
    }

    #[test]
    fn surface_spacing_cost_cramped() {
        // cramped_units([5,7,10],20) = 3.0; cost = 3 * 120 = 360
        assert_eq!(surface_spacing_cost(&[5.0, 7.0, 10.0], 20.0), 360.0);
    }

    // -----------------------------------------------------------------------
    // Pass C3 — sharedSegmentCountInvolving, crossingCountInvolving
    // -----------------------------------------------------------------------

    #[test]
    fn shared_segment_count_involving_overlapping_same_line() {
        // Two routes sharing the same horizontal segment: count = 1.
        // Route A: [{x:0,y:0},{x:100,y:0}]   Route B: [{x:0,y:0},{x:100,y:0}]
        // shared segment length = 100 > 1 → total = 1.
        let ra = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let rb = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        rbid.insert("a".to_string(), ra);
        rbid.insert("b".to_string(), rb);
        assert_eq!(shared_segment_count_involving(&rbid, &["a".to_string(), "b".to_string()]), 1);
    }

    #[test]
    fn shared_segment_count_involving_no_overlap() {
        // Two parallel horizontal routes at different y → no shared segment.
        let ra = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let rb = mk_route(vec![Point { x: 0.0, y: 10.0 }, Point { x: 100.0, y: 10.0 }]);
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        rbid.insert("a".to_string(), ra);
        rbid.insert("b".to_string(), rb);
        assert_eq!(shared_segment_count_involving(&rbid, &["a".to_string()]), 0);
    }

    #[test]
    fn crossing_count_involving_crossing_routes() {
        // Route A horizontal x:0..100 y=50, route B vertical y:0..100 x=50.
        // They cross at (50,50) → 1 crossing, touching one id → total = 1.
        let ra = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let rb = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 100.0 }]);
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        rbid.insert("a".to_string(), ra);
        rbid.insert("b".to_string(), rb);
        assert_eq!(crossing_count_involving(&rbid, &["a".to_string()]), 1);
    }

    #[test]
    fn crossing_count_involving_no_crossing() {
        // Two horizontal parallel routes → no crossings.
        let ra = mk_route(vec![Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }]);
        let rb = mk_route(vec![Point { x: 0.0, y: 10.0 }, Point { x: 100.0, y: 10.0 }]);
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        rbid.insert("a".to_string(), ra);
        rbid.insert("b".to_string(), rb);
        assert_eq!(crossing_count_involving(&rbid, &["a".to_string()]), 0);
    }

    #[test]
    fn crossing_count_involving_dedup_between_two_affected() {
        // Both routes in ids and they cross each other → counted once (not twice).
        let ra = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let rb = mk_route(vec![Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 100.0 }]);
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        rbid.insert("a".to_string(), ra);
        rbid.insert("b".to_string(), rb);
        // Both in ids: dedup means the pair (a,b) is counted once.
        assert_eq!(crossing_count_involving(&rbid, &["a".to_string(), "b".to_string()]), 1);
    }

    // -----------------------------------------------------------------------
    // Pass C3 — distribute_surface_mount_units / straighten_self_crossing_pairs
    // basic smoke tests (no mutations expected for no-op cases)
    // -----------------------------------------------------------------------

    #[test]
    fn distribute_surface_mount_units_no_flow_routes_is_noop() {
        // With no flow routes, function should return without panic.
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        let rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        let node_rects: IndexMap<String, MountRect> = IndexMap::new();
        let lane_index: IndexMap<String, i64> = IndexMap::new();
        let row_index: IndexMap<String, i64> = IndexMap::new();
        let input = MountInput {
            visible_node_ids: &[],
            node_rects: &node_rects,
            lane_index_by_node: &lane_index,
            row_index_by_node: &row_index,
            canvas_width: 1000.0,
            canvas_height: 1000.0,
        };
        distribute_surface_mount_units(&mut rbid, &rel_by_id, &input);
        assert!(rbid.is_empty());
    }

    // -----------------------------------------------------------------------
    // try_side_moves — builder receives empty route map (JS parity)
    // -----------------------------------------------------------------------

    /// JS `trySideMoves` calls `buildRouteForSides(rel, start, end)` with NO
    /// `currentRoutes` argument, so the sideRouteIndex inside the builder is
    /// empty. The Rust port must pass an empty map to `builder.build()` so the
    /// planner explores candidates without obstacle bias.
    ///
    /// This test records the largest `route_by_id` the builder ever sees.
    /// If the fix regresses, the builder would be called with the live map
    /// (containing the other route) and `max_seen` would be non-zero.
    #[test]
    fn try_side_moves_calls_builder_with_empty_route_map() {
        use std::cell::Cell;
        use relief::try_side_moves;

        struct RecordingBuilder<'a> {
            max_seen: &'a Cell<usize>,
            replacement: RouteData,
        }
        impl<'a> BuildRouteForSides for RecordingBuilder<'a> {
            fn build(
                &self,
                _rel: &MountRelationship,
                _start: &str,
                _end: &str,
                route_by_id: &IndexMap<String, RouteData>,
            ) -> Option<RouteData> {
                let n = route_by_id.len();
                if n > self.max_seen.get() {
                    self.max_seen.set(n);
                }
                Some(self.replacement.clone())
            }
        }

        // Two routes: A(0,25)→(100,25) exits right of node-A (0,20,10,10),
        //             B(90,25)→(0,25) exits left of node-B (90,20,10,10).
        // A is the flow edge under test; B is a bystander in the same diagram.
        // Before the fix, the bystander appeared in route_by_id when the builder
        // was called for A's side moves. After the fix it must not.
        let route_a = mk_route(vec![Point { x: 10.0, y: 25.0 }, Point { x: 90.0, y: 25.0 }]);
        let route_b = mk_route(vec![Point { x: 90.0, y: 25.0 }, Point { x: 10.0, y: 25.0 }]);

        // Replacement route the builder returns — must keep A on a valid side so
        // the cost check inside try_side_moves doesn't simply restore.
        // We return the same route as-is; cost guard will restore it, but the
        // builder is still called (and its route_by_id is what we assert on).
        let replacement = route_a.clone();

        let mut route_by_id: IndexMap<String, RouteData> = IndexMap::new();
        route_by_id.insert("a".to_string(), route_a.clone());
        route_by_id.insert("b".to_string(), route_b.clone());

        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("a".to_string(), mk_rel("a", "node-a", "node-b"));
        rel_by_id.insert("b".to_string(), mk_rel("b", "node-b", "node-a"));

        let mut node_rects: IndexMap<String, MountRect> = IndexMap::new();
        node_rects.insert("node-a".to_string(), mk_rect(0.0, 20.0, 10.0, 10.0));
        node_rects.insert("node-b".to_string(), mk_rect(90.0, 20.0, 10.0, 10.0));

        let visible = vec!["node-a".to_string(), "node-b".to_string()];
        let lane_idx: IndexMap<String, i64> = IndexMap::new();
        let row_idx: IndexMap<String, i64> = IndexMap::new();
        let input = MountInput {
            visible_node_ids: &visible,
            node_rects: &node_rects,
            lane_index_by_node: &lane_idx,
            row_index_by_node: &row_idx,
            canvas_width: 200.0,
            canvas_height: 100.0,
        };

        let max_seen = Cell::new(0usize);
        let builder = RecordingBuilder { max_seen: &max_seen, replacement };

        try_side_moves(&mut route_by_id, &rel_by_id, &input, Some(&builder));

        assert_eq!(
            max_seen.get(),
            0,
            "try_side_moves must call builder with an empty route map (JS parity: \
             trySideMoves calls buildRouteForSides without currentRoutes)"
        );
    }

    #[test]
    fn straighten_self_crossing_pairs_no_pairs_is_noop() {
        // With a single flow route, no pair exists → no-op.
        let ra = mk_route(vec![Point { x: 0.0, y: 50.0 }, Point { x: 100.0, y: 50.0 }]);
        let mut rbid: IndexMap<String, RouteData> = IndexMap::new();
        rbid.insert("a".to_string(), ra.clone());
        let mut rel_by_id: IndexMap<String, MountRelationship> = IndexMap::new();
        rel_by_id.insert("a".to_string(), mk_rel("a", "n1", "n2"));
        let node_rects: IndexMap<String, MountRect> = IndexMap::new();
        let lane_index: IndexMap<String, i64> = IndexMap::new();
        let row_index: IndexMap<String, i64> = IndexMap::new();
        let input = MountInput {
            visible_node_ids: &[],
            node_rects: &node_rects,
            lane_index_by_node: &lane_index,
            row_index_by_node: &row_index,
            canvas_width: 1000.0,
            canvas_height: 1000.0,
        };
        straighten_self_crossing_pairs(&mut rbid, &rel_by_id, &input);
        // Route unchanged.
        assert_eq!(rbid["a"].points, ra.points);
    }
}
