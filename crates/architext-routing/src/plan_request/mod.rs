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
pub mod c4_layout;
pub mod relationship_label;
pub mod plan_key;

use std::f64::consts::SQRT_2;
use indexmap::IndexMap;

use crate::plan_diagram::{ExtraNodeRect, SideAnchorsInput, PlanDiagramInput, LaneInput, ViewInput, RelationshipInput};
use crate::model::Point;

use diagram_layout::{DiagramLayout, LayoutConfig, diagram_layout_for};
use relationship_label::{LabelNode, relationship_label};
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

/// Route-id prefix marking a decision diamond's anchoring stem. The viewer
/// detects stems by this prefix to render them without an arrowhead (a stem is
/// an anchor, not a directional message). Shared so the producer (here) and the
/// consumer (the viewer's render model) cannot drift.
pub const DECISION_STEM_PREFIX: &str = "decision-stem:";

/// The route id for a decision diamond's anchoring stem. Distinct from the
/// decision step's own id and from any branch step id, so the viewer can detect
/// the stem (it carries no flow-step number and renders without an arrowhead).
pub fn decision_stem_id(step_id: &str) -> String {
    format!("{DECISION_STEM_PREFIX}{step_id}")
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
    /// The id of the `decision`-kind flow step this diamond was built from. Used
    /// to id the anchoring stem (`decision-stem:<step>`) and to share the step's
    /// selection highlight with the stem.
    pub decision_step_id: String,
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
            decision_step_id: step.id.clone(),
            lane_index,
            row_index,
            rect_x: x,
            rect_y: y,
            rect_width: DECISION_SIZE,
            rect_height: DECISION_SIZE,
        })
    }).collect()
}

/// Build the anchoring STEMS for a flow's decision diamonds: one short routed
/// connector per diamond, from the host node (the component that makes the
/// decision, `dn.component_id`) down to the diamond (`dn.id`). Without it the
/// diamond floats unconnected (see viewer/DESIGN.md "Flow decision diamonds").
///
/// A stem is unlabeled (no number badge — `display_index: 0`, `label: None`) and
/// routes host-bottom → diamond-top. Because a diamond's center-x equals its
/// host's center-x (`DECISION_X_OFFSET == DECISION_SIZE / 2`), the stem is a
/// clean straight vertical, not a forbidden Z/dogleg. The viewer detects the
/// stem by its `decision-stem:` route id and draws it without an arrowhead.
pub fn build_decision_stems(decision_nodes: &[DecisionNode], flow_id: &str) -> Vec<BuiltRelationship> {
    decision_nodes.iter().map(|dn| BuiltRelationship {
        id: decision_stem_id(&dn.decision_step_id),
        from: dn.component_id.clone(),
        to: dn.id.clone(),
        label: None,
        relationship_type: "stem".to_string(),
        step_id: dn.decision_step_id.clone(),
        flow_id: flow_id.to_string(),
        display_index: 0,
        kind: Some("stem".to_string()),
        return_of: None,
        outcome: None,
        preferred_start_side: Some("bottom".to_string()),
        preferred_end_side: Some("top".to_string()),
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
    let mut relationships = build_flow_relationships(flow, view);
    // Layout density is derived from the flow relationships ONLY; the stems added
    // below are anchoring decorations, not flow steps, so they must not perturb it.
    let layout = diagram_layout_for(view, relationships.len(), layout_config);
    let decision_nodes = build_decision_nodes(flow, view, &layout);

    // Anchor each decision diamond to its host node with a STEM (see
    // `build_decision_stems`). Appended after layout so stems don't perturb
    // layout density, before the cache key so the key reflects them.
    relationships.extend(build_decision_stems(&decision_nodes, &flow.id));

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

/// A node, as the structural-relationship builder needs it: id, C4 role `type`,
/// and the list of node ids it depends on.
///
/// This is the routing-side mirror of the viewer `Node` (and the JS `ArchNode`):
/// only the fields the structural edge + label rules read are carried.
pub struct StructuralNode {
    pub id: String,
    pub node_type: String,
    pub dependencies: Vec<String>,
}

/// The result of building a structural (C4 / deployment) plan request.
#[derive(Debug)]
pub struct StructuralPlanRequest {
    /// The canonical key string (sha256'd to produce the cache hash).
    pub key: String,
    /// The plan diagram input ready to pass to `plan_diagram`.
    pub plan_diagram_input: PlanDiagramInput,
    /// Edge id → display label, so the renderer can label structural edges
    /// (which carry no numbered flow step). Keyed by relationship id (`from-to`).
    pub edge_labels: IndexMap<String, String>,
}

/// The number of structural relationships a view would produce — the count the
/// deployment layout's dense-topology heuristic needs BEFORE the layout (and
/// thus the full request) is built. Uses the same visible-node + visible-dep
/// filtering as `build_structural_plan_request`, so the two cannot drift.
pub fn structural_relationship_count(view: &View, nodes: &[StructuralNode]) -> usize {
    let nodes_by_id: IndexMap<&str, &StructuralNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut visible: Vec<&str> = Vec::new();
    for lane in &view.lanes {
        for nid in &lane.node_ids {
            if seen.insert(nid.as_str()) {
                visible.push(nid.as_str());
            }
        }
    }
    let visible_set: std::collections::HashSet<&str> = visible.iter().copied().collect();
    visible.iter().map(|nid| {
        nodes_by_id.get(nid)
            .map(|n| n.dependencies.iter().filter(|d| visible_set.contains(d.as_str())).count())
            .unwrap_or(0)
    }).sum()
}

/// Port of JS structural-relationship building (`SystemMap`/`C4Diagram` in
/// `main.tsx`): for each visible node, emit one relationship per dependency
/// whose target is ALSO visible in the view. Edges are labelled by
/// `relationship_label(from, to)` — never numbered.
///
/// `view` supplies lane membership + type; `nodes` supplies dependencies + type;
/// `layout` is the C4 layout (for C4 views) or the default layout (deployment).
/// The engine pipeline and `plan_diagram` are reused unchanged — only the
/// relationships and layout differ from the flow path.
pub fn build_structural_plan_request(
    view: &View,
    nodes: &[StructuralNode],
    layout: &DiagramLayout,
    style: &str,
) -> StructuralPlanRequest {
    let nodes_by_id: IndexMap<&str, &StructuralNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Visible node ids in lane order, deduplicated (JS: `Array.from(new Set(...))`
    // for C4; `new Set(view.lanes.flatMap(...))` for deployment — both dedupe and
    // preserve first-seen order).
    let mut visible_node_ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for lane in &view.lanes {
        for nid in &lane.node_ids {
            if seen.insert(nid.clone()) {
                visible_node_ids.push(nid.clone());
            }
        }
    }
    let visible_set: std::collections::HashSet<&str> =
        visible_node_ids.iter().map(String::as_str).collect();

    // Build structural relationships: iterate visible nodes in order, emit one
    // per visible dependency. Mirrors the JS `flatMap` over visibleNodeIds.
    let mut relationships: Vec<RelationshipInput> = Vec::new();
    for nid in &visible_node_ids {
        let Some(node) = nodes_by_id.get(nid.as_str()) else { continue };
        for dep in &node.dependencies {
            if !visible_set.contains(dep.as_str()) {
                continue;
            }
            let to_node = nodes_by_id.get(dep.as_str()).copied();
            let from_label = LabelNode { id: &node.id, node_type: &node.node_type };
            let to_label = to_node.map(|t| LabelNode { id: &t.id, node_type: &t.node_type });
            let label = relationship_label(Some(&from_label), to_label.as_ref());
            relationships.push(RelationshipInput {
                id: format!("{nid}-{dep}"),
                from: nid.clone(),
                to: dep.clone(),
                label: Some(label),
                relationship_type: Some("structural".to_string()),
                step_id: None,
                flow_id: None,
                kind: None,
                return_of: None,
                outcome: None,
                display_index: 0,
                preferred_start_side: None,
                preferred_end_side: None,
            });
        }
    }

    let edge_labels: IndexMap<String, String> = relationships
        .iter()
        .filter_map(|r| r.label.clone().map(|l| (r.id.clone(), l)))
        .collect();

    // Plan key (structural relationships carry no decision rects).
    let key_lanes: Vec<PlanKeyLane<'_>> = view.lanes.iter()
        .map(|l| PlanKeyLane { id: &l.id, node_ids: &l.node_ids })
        .collect();
    let key_rels: Vec<PlanKeyRelationship> = relationships.iter().map(|r| PlanKeyRelationship {
        id: r.id.clone(),
        from: r.from.clone(),
        to: r.to.clone(),
        label: r.label.clone(),
        relationship_type: r.relationship_type.clone(),
        step_id: r.step_id.clone(),
        flow_id: r.flow_id.clone(),
        kind: r.kind.clone(),
        return_of: r.return_of.clone(),
        outcome: r.outcome.clone(),
        display_index: r.display_index,
        preferred_start_side: r.preferred_start_side.clone(),
        preferred_end_side: r.preferred_end_side.clone(),
    }).collect();
    // Structural visibleNodeIds are deduped already; the key sorts them.
    let visible_sorted = sorted_visible_node_ids(visible_node_ids.iter().cloned());
    let key = plan_input_key(&PlanKeyInput {
        view_id: &view.id,
        view_type: &view.view_type,
        lanes: &key_lanes,
        relationships: &key_rels,
        visible_node_ids: &visible_sorted,
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
        extra_node_rects: &[],
        extra_lane_index_by_node: &[],
        extra_row_index_by_node: &[],
        score_edge_proximity: false,
        style,
    });

    let plan_diagram_input = PlanDiagramInput {
        view: ViewInput {
            lanes: view.lanes.iter().map(|l| LaneInput {
                id: l.id.clone(),
                node_ids: l.node_ids.clone(),
            }).collect(),
        },
        relationships,
        visible_node_ids,
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
        extra_node_rects: IndexMap::new(),
        extra_lane_index_by_node: IndexMap::new(),
        extra_row_index_by_node: IndexMap::new(),
        score_edge_proximity: false,
        style: style.to_string(),
        diagnostics: false,
    };

    StructuralPlanRequest { key, plan_diagram_input, edge_labels }
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

#[cfg(test)]
mod stem_tests {
    use super::*;

    fn decision_node(id: &str, component_id: &str, step_id: &str) -> DecisionNode {
        DecisionNode {
            id: id.to_string(),
            component_id: component_id.to_string(),
            decision_step_id: step_id.to_string(),
            lane_index: 0,
            row_index: 0,
            rect_x: 0.0,
            rect_y: 0.0,
            rect_width: DECISION_SIZE,
            rect_height: DECISION_SIZE,
        }
    }

    #[test]
    fn stem_anchors_host_to_diamond_unlabeled_vertical() {
        // Mirrors the FlowForge `fresh-install` flow: the `validate-install`
        // decision is hosted by `schema-validator`, diamond id `decision:...`.
        let dn = decision_node("decision:validate-install", "schema-validator", "validate-install");
        let stems = build_decision_stems(std::slice::from_ref(&dn), "fresh-install");

        assert_eq!(stems.len(), 1, "one stem per decision diamond");
        let s = &stems[0];
        // Host (the deciding component) → diamond, so the diamond stops floating.
        assert_eq!(s.from, "schema-validator");
        assert_eq!(s.to, "decision:validate-install");
        // No number badge: a stem is an anchor, not a numbered flow step.
        assert!(s.label.is_none());
        assert_eq!(s.display_index, 0);
        assert_eq!(s.relationship_type, "stem");
        assert_eq!(s.kind.as_deref(), Some("stem"));
        // Routes straight down the host's bottom into the diamond's top tip.
        assert_eq!(s.preferred_start_side.as_deref(), Some("bottom"));
        assert_eq!(s.preferred_end_side.as_deref(), Some("top"));
        // Shares the decision step's id so it highlights with that step.
        assert_eq!(s.step_id, "validate-install");
        // Id is distinct from the decision step id and any branch step id.
        assert_eq!(s.id, "decision-stem:validate-install");
    }

    #[test]
    fn no_decisions_yields_no_stems() {
        assert!(build_decision_stems(&[], "flow").is_empty());
    }
}
