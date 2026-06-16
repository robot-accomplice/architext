//! Port of `viewer/src/presentation/decisionBranchModel.js`.
//!
//! Pure side-selection logic for decision-diamond branches. No JS-compat rounding
//! needed here — all operations are comparisons on integer-valued lane/row indices.

/// Lane + row position of a node within a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanePosition {
    pub lane_index: usize,
    pub row_index: usize,
}

/// Return the (laneIndex, rowIndex) of `nodeId` within `view`, or `None` if not found.
pub fn node_lane_position(lanes: &[crate::plan_request::types::Lane], node_id: &str) -> Option<LanePosition> {
    for (lane_index, lane) in lanes.iter().enumerate() {
        if let Some(row_index) = lane.node_ids.iter().position(|id| id == node_id) {
            return Some(LanePosition { lane_index, row_index });
        }
    }
    None
}

/// Which side of a decision diamond a branch toward `target_node_id` leaves from.
///
/// Port of JS `preferredDecisionBranchSide`.
pub fn preferred_decision_branch_side(
    lanes: &[crate::plan_request::types::Lane],
    decision_position: LanePosition,
    target_node_id: &str,
) -> &'static str {
    let target = match node_lane_position(lanes, target_node_id) {
        Some(t) => t,
        None => return "right",
    };
    let delta_lane = target.lane_index as i64 - decision_position.lane_index as i64;
    let delta_row = target.row_index as i64 - decision_position.row_index as i64;
    if delta_lane > 0 {
        if delta_row > 0 { "bottom" } else { "right" }
    } else if delta_lane < 0 || delta_row < 0 {
        // Backward lane OR same-lane-but-earlier-row both exit left.
        "left"
    } else if delta_row > 0 {
        "bottom"
    } else {
        "right"
    }
}

/// Which face of the target node a branch enters.
///
/// Port of JS `preferredDecisionBranchEndSide`.
pub fn preferred_decision_branch_end_side(
    lanes: &[crate::plan_request::types::Lane],
    decision_position: LanePosition,
    target_node_id: &str,
    start_side: &str,
) -> &'static str {
    if let Some(target) = node_lane_position(lanes, target_node_id) {
        if target.lane_index > decision_position.lane_index {
            return "left";
        }
        if target.lane_index < decision_position.lane_index {
            return "right";
        }
        if target.row_index < decision_position.row_index {
            return "left";
        }
        if target.row_index > decision_position.row_index {
            return "top";
        }
    }
    opposite_side(start_side)
}

fn opposite_side(side: &str) -> &'static str {
    match side {
        "left" => "right",
        "right" => "left",
        "top" => "bottom",
        _ => "top",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_request::types::Lane;

    fn lanes() -> Vec<Lane> {
        vec![
            Lane { id: "l0".to_string(), node_ids: vec!["a".to_string(), "b".to_string()] },
            Lane { id: "l1".to_string(), node_ids: vec!["c".to_string(), "d".to_string()] },
            Lane { id: "l2".to_string(), node_ids: vec!["e".to_string()] },
        ]
    }

    #[test]
    fn node_lane_position_found() {
        let l = lanes();
        assert_eq!(node_lane_position(&l, "c"), Some(LanePosition { lane_index: 1, row_index: 0 }));
        assert_eq!(node_lane_position(&l, "d"), Some(LanePosition { lane_index: 1, row_index: 1 }));
    }

    #[test]
    fn node_lane_position_not_found() {
        let l = lanes();
        assert_eq!(node_lane_position(&l, "x"), None);
    }

    #[test]
    fn branch_side_forward_right() {
        // target is in a later lane at same row → "right"
        let l = lanes();
        let dp = LanePosition { lane_index: 0, row_index: 0 };
        assert_eq!(preferred_decision_branch_side(&l, dp, "c"), "right");
    }

    #[test]
    fn branch_side_forward_down_bottom() {
        // target is in a later lane at a later row → "bottom"
        let l = lanes();
        let dp = LanePosition { lane_index: 0, row_index: 0 };
        assert_eq!(preferred_decision_branch_side(&l, dp, "d"), "bottom");
    }

    #[test]
    fn branch_side_backward() {
        // target is in an earlier lane → "left"
        let l = lanes();
        let dp = LanePosition { lane_index: 1, row_index: 0 };
        assert_eq!(preferred_decision_branch_side(&l, dp, "a"), "left");
    }

    #[test]
    fn branch_end_side_forward_lane() {
        let l = lanes();
        let dp = LanePosition { lane_index: 0, row_index: 0 };
        assert_eq!(preferred_decision_branch_end_side(&l, dp, "c", "right"), "left");
    }
}
