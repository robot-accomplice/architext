//! Port of the plan-request building layer.
//!
//! This module corresponds to the JS files:
//! - `viewer/src/presentation/planRequest.js` → `build_flow_plan_request`
//! - `viewer/src/routing/planKey.js` → `plan_input_key` (in `plan_key`)
//! - `viewer/src/presentation/viewSelection.js` (subset) → `view_selection`
//! - `viewer/src/presentation/diagramLayout.js` → `diagram_layout`
//! - `viewer/src/presentation/flowStepDisplayModel.js` → `flow_step_display`
//! - `viewer/src/presentation/decisionBranchModel.js` → `decision_branch`

pub mod types;
pub mod decision_branch;
pub mod flow_step_display;
pub mod view_selection;
pub mod diagram_layout;
pub mod plan_key;

use std::f64::consts::SQRT_2;
use indexmap::IndexMap;

use crate::plan_diagram::{ExtraNodeRect, SideAnchorsInput, PlanDiagramInput, LaneInput, ViewInput, RelationshipInput};
use crate::model::Point;

use diagram_layout::{DiagramLayout, LayoutConfig, diagram_layout_for};
use decision_branch::{node_lane_position, preferred_decision_branch_side, preferred_decision_branch_end_side};
use flow_step_display::{flow_step_display_indexes, decision_branch_targets};
use plan_key::{
    PlanKeyInput, PlanKeyLane, PlanKeyRelationship, PlanKeyExtraRect, PlanKeyExtraIndex,
    plan_input_key, round_rect, sorted_visible_node_ids,
};
use types::{Flow, View};

// Decision rect offsets — named constants matching JS magic numbers.
const DECISION_X_OFFSET: f64 = 19.0;
const DECISION_Y_OFFSET: f64 = 22.0;
const DECISION_SIZE: f64 = 38.0;

/// Port of JS `decisionNodeId(stepId)`.
fn decision_node_id(step_id: &str) -> String {
    format!("decision:{step_id}")
}

/// Port of JS `decisionTip(rect, side)`.
/// Returns the tip point of a diamond at the given side.
fn decision_tip(x: f64, y: f64, width: f64, height: f64, side: &str) -> Point {
    let center_x = x + width / 2.0;
    let center_y = y + height / 2.0;
    let radius = width / SQRT_2;
    match side {
        "left"  => Point { x: center_x - radius, y: center_y },
        "right" => Point { x: center_x + radius, y: center_y },
        "top"   => Point { x: center_x, y: center_y - radius },
        _       => Point { x: center_x, y: center_y + radius }, // "bottom"
    }
}

/// A decision node — an augmented routing rect placed below its affiliated component.
#[derive(Debug, Clone)]
pub struct DecisionNode {
    pub id: String,
    pub component_id: String,
    pub lane_index: usize,
    pub row_index: usize,
    pub rect_x: f64,
    pub rect_y: f64,
    pub rect_width: f64,
    pub rect_height: f64,
}

/// A built relationship, ready for planKey construction and plan engine input.
#[derive(Debug, Clone)]
pub struct BuiltRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub relationship_type: String,
    pub step_id: String,
    pub flow_id: String,
    pub display_index: i64,
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
}

/// Port of JS `buildFlowRelationships(flow, view)`.
pub fn build_flow_relationships(flow: &Flow, view: &View) -> Vec<BuiltRelationship> {
    let display_indexes = flow_step_display_indexes(&flow.steps);
    // Map from `to` node → the decision step that targets it (last wins, JS Map semantics).
    let decision_step_by_target: IndexMap<&str, &crate::plan_request::types::FlowStep> = flow.steps.iter()
        .filter(|s| s.kind.as_deref() == Some("decision"))
        .map(|s| (s.to.as_str(), s))
        .collect();

    flow.steps.iter().enumerate().map(|(index, step)| {
        let display_index = display_indexes.get(&step.id).copied().unwrap_or(index + 1) as i64;
        let decision_step = if step.outcome.is_some() {
            decision_step_by_target.get(step.from.as_str()).copied()
        } else {
            None
        };
        let decision_position = decision_step.and_then(|_| node_lane_position(&view.lanes, &step.from));
        let branch_start_side = match (decision_step, decision_position) {
            (Some(_), Some(pos)) => Some(preferred_decision_branch_side(&view.lanes, pos, &step.to).to_string()),
            _ => None,
        };
        let branch_end_side = match (&branch_start_side, decision_position) {
            (Some(start), Some(pos)) => Some(preferred_decision_branch_end_side(&view.lanes, pos, &step.to, start).to_string()),
            _ => None,
        };

        BuiltRelationship {
            id: step.id.clone(),
            from: match decision_step {
                Some(ds) => decision_node_id(&ds.id),
                None => step.from.clone(),
            },
            to: step.to.clone(),
            label: Some(format!("{}. {}", display_index, step.action)),
            relationship_type: "flow".to_string(),
            step_id: match decision_step {
                Some(ds) => ds.id.clone(),
                None => step.id.clone(),
            },
            flow_id: flow.id.clone(),
            display_index,
            kind: step.kind.clone(),
            return_of: step.return_of.clone(),
            outcome: step.outcome.clone(),
            preferred_start_side: branch_start_side,
            preferred_end_side: branch_end_side,
        }
    }).collect()
}

/// Port of JS `buildDecisionNodes(flow, view, layout)`.
pub fn build_decision_nodes(flow: &Flow, view: &View, layout: &DiagramLayout) -> Vec<DecisionNode> {
    let branched_targets = decision_branch_targets(&flow.steps);
    let display_indexes = flow_step_display_indexes(&flow.steps);

    flow.steps.iter().filter(|step| {
        step.kind.as_deref() == Some("decision") && branched_targets.contains(&step.to)
    }).filter_map(|step| {
        let position = node_lane_position(&view.lanes, &step.to)?;
        let lane_index = position.lane_index;
        let row_index = position.row_index;
        let x = layout.margin_x + lane_index as f64 * layout.lane_width + layout.node_width / 2.0 - DECISION_X_OFFSET;
        let y = layout.margin_y + row_index as f64 * layout.row_gap + layout.node_height + DECISION_Y_OFFSET;
        let _ = display_indexes.get(&step.id).copied().unwrap_or(0);
        Some(DecisionNode {
            id: decision_node_id(&step.id),
            component_id: step.to.clone(),
            lane_index,
            row_index,
            rect_x: x,
            rect_y: y,
            rect_width: DECISION_SIZE,
            rect_height: DECISION_SIZE,
        })
    }).collect()
}

/// The result of building a flow plan request.
#[derive(Debug)]
pub struct FlowPlanRequest {
    /// The canonical key string (sha256'd to produce the cache hash).
    pub key: String,
    /// The plan diagram input ready to pass to `plan_diagram`.
    pub plan_diagram_input: PlanDiagramInput,
}

/// Port of JS `buildFlowPlanRequest({ view, flow, layoutConfig, style })`.
///
/// Builds the full plan request: relationships, decision nodes, layout,
/// and the assembled `PlanDiagramInput`.
pub fn build_flow_plan_request(
    view: &View,
    flow: &Flow,
    layout_config: Option<&LayoutConfig>,
    style: &str,
) -> FlowPlanRequest {
    let relationships = build_flow_relationships(flow, view);
    let layout = diagram_layout_for(view, relationships.len(), layout_config);
    let decision_nodes = build_decision_nodes(flow, view, &layout);

    // visibleNodeIds = union of all nodeIds across all lanes
    let visible_node_ids_unsorted: Vec<String> = view.lanes.iter()
        .flat_map(|l| l.node_ids.iter().cloned())
        .collect();
    let visible_node_ids_sorted = sorted_visible_node_ids(visible_node_ids_unsorted.iter().cloned());

    // Build the plan key input
    let key_lanes: Vec<PlanKeyLane<'_>> = view.lanes.iter()
        .map(|l| PlanKeyLane { id: &l.id, node_ids: &l.node_ids })
        .collect();

    let key_rels: Vec<PlanKeyRelationship> = relationships.iter().map(|r| PlanKeyRelationship {
        id: r.id.clone(),
        from: r.from.clone(),
        to: r.to.clone(),
        label: r.label.clone(),
        relationship_type: Some(r.relationship_type.clone()),
        step_id: Some(r.step_id.clone()),
        flow_id: Some(r.flow_id.clone()),
        kind: r.kind.clone(),
        return_of: r.return_of.clone(),
        outcome: r.outcome.clone(),
        display_index: r.display_index,
        preferred_start_side: r.preferred_start_side.clone(),
        preferred_end_side: r.preferred_end_side.clone(),
    }).collect();

    // extraNodeRects: decision nodes, sorted by id (locale compare)
    let mut extra_rects_entries: Vec<(String, &DecisionNode)> = decision_nodes.iter()
        .map(|n| (n.id.clone(), n))
        .collect();
    extra_rects_entries.sort_by(|(a, _), (b, _)| crate::js_compat::js_locale_compare(a, b));

    let key_extra_rects: Vec<PlanKeyExtraRect> = extra_rects_entries.iter().map(|(id, node)| {
        PlanKeyExtraRect {
            node_id: id.clone(),
            rounded: round_rect(node.rect_x, node.rect_y, node.rect_width, node.rect_height),
        }
    }).collect();

    // extraLaneIndexByNode, sorted by node_id
    let mut lane_idx_entries: Vec<(String, usize)> = decision_nodes.iter()
        .map(|n| (n.id.clone(), n.lane_index))
        .collect();
    lane_idx_entries.sort_by(|(a, _), (b, _)| crate::js_compat::js_locale_compare(a, b));
    let key_extra_lane: Vec<PlanKeyExtraIndex> = lane_idx_entries.iter().map(|(id, idx)| PlanKeyExtraIndex {
        node_id: id.clone(),
        value: *idx as i64,
    }).collect();

    // extraRowIndexByNode, sorted by node_id
    let mut row_idx_entries: Vec<(String, usize)> = decision_nodes.iter()
        .map(|n| (n.id.clone(), n.row_index))
        .collect();
    row_idx_entries.sort_by(|(a, _), (b, _)| crate::js_compat::js_locale_compare(a, b));
    let key_extra_row: Vec<PlanKeyExtraIndex> = row_idx_entries.iter().map(|(id, idx)| PlanKeyExtraIndex {
        node_id: id.clone(),
        value: *idx as i64,
    }).collect();

    let key = plan_input_key(&PlanKeyInput {
        view_id: &view.id,
        view_type: &view.view_type,
        lanes: &key_lanes,
        relationships: &key_rels,
        visible_node_ids: &visible_node_ids_sorted,
        node_width: layout.node_width,
        node_height: layout.node_height,
        lane_width: layout.lane_width,
        row_gap: layout.row_gap,
        margin_x: layout.margin_x,
        margin_y: layout.margin_y,
        min_canvas_width: layout.min_canvas_width,
        min_canvas_height: layout.min_canvas_height,
        canvas_extra_width: layout.canvas_extra_width,
        canvas_extra_height: layout.canvas_extra_height,
        extra_node_rects: &key_extra_rects,
        extra_lane_index_by_node: &key_extra_lane,
        extra_row_index_by_node: &key_extra_row,
        score_edge_proximity: false,
        style,
    });

    // Build PlanDiagramInput for the plan engine
    let plan_diagram_input = build_plan_diagram_input(
        view,
        &relationships,
        &visible_node_ids_unsorted,
        &layout,
        &decision_nodes,
        style,
    );

    FlowPlanRequest { key, plan_diagram_input }
}

/// Assemble the `PlanDiagramInput` that the Rust plan engine accepts.
/// Port of JS `assemblePlanInput(...)`.
fn build_plan_diagram_input(
    view: &View,
    relationships: &[BuiltRelationship],
    visible_node_ids: &[String],
    layout: &DiagramLayout,
    decision_nodes: &[DecisionNode],
    style: &str,
) -> PlanDiagramInput {
    // Convert view lanes
    let view_input = ViewInput {
        lanes: view.lanes.iter().map(|l| LaneInput {
            id: l.id.clone(),
            node_ids: l.node_ids.clone(),
        }).collect(),
    };

    // Convert relationships
    let input_relationships: Vec<RelationshipInput> = relationships.iter().map(|r| RelationshipInput {
        id: r.id.clone(),
        from: r.from.clone(),
        to: r.to.clone(),
        label: r.label.clone(),
        relationship_type: Some(r.relationship_type.clone()),
        step_id: Some(r.step_id.clone()),
        flow_id: Some(r.flow_id.clone()),
        kind: r.kind.clone(),
        return_of: r.return_of.clone(),
        outcome: r.outcome.clone(),
        display_index: r.display_index,
        preferred_start_side: r.preferred_start_side.clone(),
        preferred_end_side: r.preferred_end_side.clone(),
    }).collect();

    // extraNodeRects: decision nodes as diamond rects with fixedPorts + sideAnchors
    let mut extra_node_rects: IndexMap<String, ExtraNodeRect> = IndexMap::new();
    let mut extra_lane_index_by_node: IndexMap<String, i64> = IndexMap::new();
    let mut extra_row_index_by_node: IndexMap<String, i64> = IndexMap::new();

    for node in decision_nodes {
        let rx = node.rect_x;
        let ry = node.rect_y;
        let rw = node.rect_width;
        let rh = node.rect_height;
        let side_anchors = SideAnchorsInput {
            left:   Some(decision_tip(rx, ry, rw, rh, "left")),
            right:  Some(decision_tip(rx, ry, rw, rh, "right")),
            top:    Some(decision_tip(rx, ry, rw, rh, "top")),
            bottom: Some(decision_tip(rx, ry, rw, rh, "bottom")),
        };
        extra_node_rects.insert(node.id.clone(), ExtraNodeRect {
            x: rx, y: ry, width: rw, height: rh,
            fixed_ports: true,
            side_anchors: Some(side_anchors),
        });
        extra_lane_index_by_node.insert(node.id.clone(), node.lane_index as i64);
        extra_row_index_by_node.insert(node.id.clone(), node.row_index as i64);
    }

    PlanDiagramInput {
        view: view_input,
        relationships: input_relationships,
        visible_node_ids: visible_node_ids.to_vec(),
        node_width: layout.node_width,
        node_height: layout.node_height,
        lane_width: layout.lane_width,
        row_gap: layout.row_gap,
        margin_x: layout.margin_x,
        margin_y: layout.margin_y,
        min_canvas_width: layout.min_canvas_width,
        min_canvas_height: layout.min_canvas_height,
        canvas_extra_width: layout.canvas_extra_width,
        canvas_extra_height: layout.canvas_extra_height,
        extra_node_rects,
        extra_lane_index_by_node,
        extra_row_index_by_node,
        score_edge_proximity: false,
        style: style.to_string(),
        diagnostics: false,
    }
}
