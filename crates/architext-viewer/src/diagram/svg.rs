//! The diagram `<svg>`: fluid viewBox, pan/zoom transform group, marker defs,
//! and composition of the node/edge/label/decision renderers.
//!
//! DESIGN.md rule 3: the SVG is FLUID — `width=100%`/`height=100%` with a
//! `viewBox` sized to the plan's canvas. It NEVER has fixed pixel w/h. Pan/zoom
//! are applied as a single `transform` on the inner `<g>` (translate + scale),
//! driven by signals owned by the canvas panel.

use std::collections::HashMap;

use architext_routing::model::{Plan, Rect};
use leptos::*;

use super::edge::{DiagramEdge, EdgeView, ARROWHEAD_ID};
use super::label::{DiagramLabel, LabelKind, LabelView};
use super::pill_placement::{place_pills, PillInput};
use super::node::{DecisionDiamond, DiagramNode, NodeView};
use super::{role_color_var, EdgeKind};
use crate::components::relationship_icon::RelationshipKind;
use crate::data::models::{Flow, Node, View as DataView};

/// The fully-resolved render model for one diagram: everything the renderers
/// need, derived once from the plan + dataset (no per-element lookups in the
/// view closures).
#[derive(Clone)]
pub struct RenderModel {
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub nodes: Vec<NodeView>,
    pub decisions: Vec<(Rect, String)>,
    pub edges: Vec<EdgeView>,
    pub labels: Vec<LabelView>,
}

/// The legend payload, derived from ONLY what the current view renders: the
/// node types that appear (swatch + glyph + name) and, when structural edges
/// are shown, the relationship kinds that appear (glyph + word). Empty lists
/// mean "nothing of that family in this view" and the legend omits the section.
#[derive(Clone, Default, PartialEq)]
pub struct LegendModel {
    /// Distinct node types present, in first-seen order (the authored `type`).
    pub node_types: Vec<String>,
    /// Distinct relationship kinds present (structural diagrams only).
    pub relationship_kinds: Vec<RelationshipKind>,
}

/// Build the render model from a computed `Plan`, the selected flow (for edge
/// kinds, flows mode only) and view, and the node registry (for names + C4
/// types).
///
/// `flow` is `None` in structural (C4 / deployment) mode — there are no flow
/// steps, so every edge is a plain `Process` edge and labels come from
/// `edge_labels` (the relationship label rule's output, keyed by route id).
/// In flows mode `flow` is `Some` and `edge_labels` is empty; labels then fall
/// back to the `"{index}. {action}"` reconstruction from the flow.
///
/// Decision rects are the `node_rects` entries whose ids are not real nodes
/// (the `decision:<step>` augmented rects); they render as diamonds and are
/// excluded from the node-card pass.
pub fn build_render_model(
    plan: &Plan,
    flow: Option<&Flow>,
    edge_labels: &HashMap<String, String>,
    nodes_by_id: &HashMap<&str, &Node>,
) -> RenderModel {
    // Edge kind per step id (the route key == the flow step id). Empty in
    // structural mode → every edge defaults to Process.
    let kind_by_step: HashMap<&str, EdgeKind> = flow
        .map(|f| f.steps.iter()
            .map(|s| (s.id.as_str(), EdgeKind::from_step_kind(s.kind.as_deref())))
            .collect())
        .unwrap_or_default();

    // Node cards: rects that resolve to a real dataset node.
    let mut node_views = Vec::new();
    let mut decisions = Vec::new();
    for (id, rect) in &plan.node_rects {
        match nodes_by_id.get(id.as_str()) {
            Some(node) => node_views.push(NodeView {
                id: id.clone(),
                name: node.name.clone(),
                node_type: node.node_type.clone(),
                rect: rect.clone(),
            }),
            None => {
                // An augmented (decision) rect — tint with the component's role
                // color when the affiliated node is resolvable, else external.
                // Structural mode has no decision rects (no flow), but guard
                // anyway: default to the external role when there is no flow.
                let component_type = flow
                    .map(|f| decision_component_type(id, f, nodes_by_id))
                    .unwrap_or("external");
                decisions.push((rect.clone(), role_color_var(component_type)));
            }
        }
    }

    // Edges: the `d`-string verbatim, with the resolved kind.
    let edges = plan
        .routes
        .iter()
        .map(|(id, route)| EdgeView {
            id: id.clone(),
            d: route.d.clone(),
            kind: kind_by_step.get(id.as_str()).copied().unwrap_or(EdgeKind::Process),
        })
        .collect();

    // Labels: anchor from the route, background from the matching label box.
    // Flow-step labels (`"N. action"`) collapse to a compact number badge — the
    // full exposition lives in the steps panel; structural labels (`"uses"`)
    // keep the box+text treatment.
    //
    // The engine's `route.label_x/label_y` is only a SEED here (see
    // `pill_placement` for why): we re-anchor each pill onto its own route
    // polyline and stagger/de-overlap the small rendered glyphs in screen space
    // (F1 flow number pills, F9 structural kind pills). We first build each
    // label with the engine seed, collect the placement inputs in the same
    // order, then overwrite the anchors with the resolved positions.
    let mut labels: Vec<LabelView> = Vec::new();
    let mut pill_inputs: Vec<PillInput> = Vec::new();
    for (id, route) in &plan.routes {
        let Some(raw_label) = route
            .extra
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| edge_labels.get(id).cloned())
            .or_else(|| flow.and_then(|f| edge_label_from_flow(id, f)))
        else {
            continue;
        };
        let collapsed = pill_label(&raw_label);
        // A flow-step label collapses to its number (a badge); anything else
        // is a structural relationship label → a glyph pill (the word kept
        // for the hover title + legend).
        let kind = if collapsed != raw_label {
            LabelKind::Number(collapsed)
        } else {
            LabelKind::Relationship {
                kind: RelationshipKind::classify(&raw_label),
                word: raw_label,
            }
        };
        let box_rect = plan.label_boxes.get(id).cloned().unwrap_or(Rect {
            x: route.label_x,
            y: route.label_y,
            width: 0.0,
            height: 0.0,
        });
        labels.push(LabelView {
            kind,
            anchor_x: route.label_x,
            anchor_y: route.label_y,
            box_rect,
        });
        pill_inputs.push(PillInput {
            route_points: route.points.clone(),
            seed: (route.label_x, route.label_y),
        });
    }
    // Anchor-on-line + stagger + de-overlap, in screen space.
    for (label, (x, y)) in labels.iter_mut().zip(place_pills(&pill_inputs)) {
        label.anchor_x = x;
        label.anchor_y = y;
    }

    RenderModel {
        canvas_width: plan.canvas_width,
        canvas_height: plan.canvas_height,
        nodes: node_views,
        decisions,
        edges,
        labels,
    }
}

/// Public legend derivation for the canvas overlay: the distinct node types of
/// the cards that will render (every visible node whose id resolves to a real
/// dataset node) and the distinct relationship kinds of the structural edge
/// labels. Flows mode passes empty `edge_labels`, so the relationship section
/// is empty there. This mirrors what `build_render_model` renders without
/// recomputing the full plan-bound model.
pub fn legend_for(
    plan: &Plan,
    edge_labels: &HashMap<String, String>,
    nodes_by_id: &HashMap<&str, &Node>,
) -> LegendModel {
    let mut node_types: Vec<String> = Vec::new();
    for id in plan.node_rects.keys() {
        if let Some(node) = nodes_by_id.get(id.as_str()) {
            if !node_types.contains(&node.node_type) {
                node_types.push(node.node_type.clone());
            }
        }
    }
    let mut relationship_kinds: Vec<RelationshipKind> = Vec::new();
    for label in edge_labels.values() {
        let kind = RelationshipKind::classify(label);
        if !relationship_kinds.contains(&kind) {
            relationship_kinds.push(kind);
        }
    }
    LegendModel { node_types, relationship_kinds }
}

/// The role-type to tint a decision diamond: the type of the component the
/// decision step targets (`decision:<stepId>` → step.to → node.type).
fn decision_component_type<'a>(
    decision_id: &str,
    flow: &Flow,
    nodes_by_id: &HashMap<&str, &'a Node>,
) -> &'a str {
    let step_id = decision_id.strip_prefix("decision:").unwrap_or(decision_id);
    flow.steps
        .iter()
        .find(|s| s.id == step_id)
        .and_then(|s| nodes_by_id.get(s.to.as_str()))
        .map(|n| n.node_type.as_str())
        .unwrap_or("external")
}

/// The on-diagram pill text. Flow-step labels arrive as `"N. action"` (e.g.
/// `"1. resolveTargetPath"`); the pill shows only the step number `N`, with the
/// full action text living in the steps-panel footer. Structural labels
/// (`"uses"`, `"depends on"` from C4 / deployment relationships) carry no
/// leading `N.`/`N)` and are returned unchanged.
fn pill_label(text: &str) -> String {
    let trimmed = text.trim_start();
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let rest = &trimmed[digits.len()..];
        if matches!(rest.chars().next(), Some('.') | Some(')')) {
            return digits;
        }
    }
    text.to_string()
}

/// Reconstruct an edge's display label from the flow when the route doesn't
/// carry one (mirrors the engine's `"{index}. {action}"` label format).
fn edge_label_from_flow(edge_id: &str, flow: &Flow) -> Option<String> {
    flow.steps
        .iter()
        .enumerate()
        .find(|(_, s)| s.id == edge_id)
        .map(|(i, s)| format!("{}. {}", i + 1, s.action))
}

/// The fluid diagram SVG. `pan_x`/`pan_y`/`zoom` drive the inner transform.
#[component]
pub fn DiagramSvg(
    plan: Plan,
    /// The selected flow (flows mode). `None` in structural (C4 / deployment)
    /// mode, where edges come from structural relationships, not flow steps.
    flow: Option<Flow>,
    /// Edge id → label, for structural mode (empty in flows mode).
    edge_labels: HashMap<String, String>,
    #[allow(unused)] view: DataView,
    nodes: Vec<Node>,
    #[prop(into)] pan_x: Signal<f64>,
    #[prop(into)] pan_y: Signal<f64>,
    #[prop(into)] zoom: Signal<f64>,
    #[prop(into)] selected_node: Signal<Option<String>>,
    /// The selected flow step id (steps-panel selection). The flow route id
    /// equals the step id, so an edge whose id matches gets the `--accent` active
    /// treatment. Always `None` in structural (C4 / deployment) mode.
    #[prop(into)] selected_step: Signal<Option<String>>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let nodes_by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let model = build_render_model(&plan, flow.as_ref(), &edge_labels, &nodes_by_id);

    let view_box = format!("0 0 {} {}", model.canvas_width, model.canvas_height);
    let transform = move || format!("translate({} {}) scale({})", pan_x.get(), pan_y.get(), zoom.get());

    let node_items = model.nodes.clone();
    let decision_items = model.decisions.clone();
    let edge_items = model.edges.clone();
    let label_items = model.labels.clone();

    view! {
        <svg class="flow-svg" viewBox=view_box preserveAspectRatio="xMidYMid meet">
            <defs>
                // Shared arrowhead marker (drawn in the edge stroke color).
                <marker
                    id=ARROWHEAD_ID
                    viewBox="0 0 10 10"
                    refX="9" refY="5"
                    markerWidth="7" markerHeight="7"
                    markerUnits="userSpaceOnUse"
                    orient="auto-start-reverse"
                >
                    <path d="M 0 0 L 10 5 L 0 10 z" class="flow-arrowhead"></path>
                </marker>
            </defs>
            <g class="flow-transform" transform=transform>
                // Z-order: edges, then labels, then decisions, then node cards on top.
                <g class="flow-edges">
                    {edge_items.into_iter().map(|e| {
                        let id = e.id.clone();
                        let is_selected = Signal::derive(move || {
                            selected_step.get().as_deref() == Some(id.as_str())
                        });
                        view! { <DiagramEdge edge=e selected=is_selected/> }
                    }).collect_view()}
                </g>
                <g class="flow-labels">
                    {label_items.into_iter().map(|l| view! { <DiagramLabel label=l/> }).collect_view()}
                </g>
                <g class="flow-decisions">
                    {decision_items.into_iter().map(|(rect, role)| view! {
                        <DecisionDiamond rect=rect role_var=role/>
                    }).collect_view()}
                </g>
                <g class="flow-nodes">
                    {node_items.into_iter().map(|n| {
                        let id = n.id.clone();
                        let is_selected = Signal::derive(move || {
                            selected_node.get().as_deref() == Some(id.as_str())
                        });
                        view! { <DiagramNode node=n selected=is_selected on_select=on_select/> }
                    }).collect_view()}
                </g>
            </g>
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::pill_label;

    #[test]
    fn flow_step_labels_collapse_to_the_step_number() {
        assert_eq!(pill_label("1. resolveTargetPath"), "1");
        assert_eq!(pill_label("12. x"), "12");
        // The `N)` outcome-branch form also collapses to its number.
        assert_eq!(pill_label("3) hit"), "3");
    }

    #[test]
    fn structural_labels_are_returned_unchanged() {
        assert_eq!(pill_label("uses"), "uses");
        assert_eq!(pill_label("depends on"), "depends on");
        // A bare number with no `.`/`)` separator is not a step label.
        assert_eq!(pill_label("7 retries"), "7 retries");
    }
}
