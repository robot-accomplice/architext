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
use super::label::{DiagramLabel, LabelView};
use super::node::{DecisionDiamond, DiagramNode, NodeView};
use super::{role_color_var, EdgeKind};
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

/// Build the render model from a computed `Plan`, the selected flow (for edge
/// kinds) and view, and the node registry (for names + C4 types).
///
/// Decision rects are the `node_rects` entries whose ids are not real nodes
/// (the `decision:<step>` augmented rects); they render as diamonds and are
/// excluded from the node-card pass.
pub fn build_render_model(
    plan: &Plan,
    flow: &Flow,
    nodes_by_id: &HashMap<&str, &Node>,
) -> RenderModel {
    // Edge kind per step id (the route key == the flow step id).
    let kind_by_step: HashMap<&str, EdgeKind> = flow
        .steps
        .iter()
        .map(|s| (s.id.as_str(), EdgeKind::from_step_kind(s.kind.as_deref())))
        .collect();

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
                let component_type = decision_component_type(id, flow, nodes_by_id);
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
    let labels = plan
        .routes
        .iter()
        .filter_map(|(id, route)| {
            let label_text = route
                .extra
                .get("label")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| edge_label_from_flow(id, flow))?;
            let box_rect = plan.label_boxes.get(id).cloned().unwrap_or(Rect {
                x: route.label_x,
                y: route.label_y,
                width: 0.0,
                height: 0.0,
            });
            Some(LabelView {
                text: label_text,
                anchor_x: route.label_x,
                anchor_y: route.label_y,
                box_rect,
            })
        })
        .collect();

    RenderModel {
        canvas_width: plan.canvas_width,
        canvas_height: plan.canvas_height,
        nodes: node_views,
        decisions,
        edges,
        labels,
    }
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
    flow: Flow,
    #[allow(unused)] view: DataView,
    nodes: Vec<Node>,
    #[prop(into)] pan_x: Signal<f64>,
    #[prop(into)] pan_y: Signal<f64>,
    #[prop(into)] zoom: Signal<f64>,
    #[prop(into)] selected_node: Signal<Option<String>>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let nodes_by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let model = build_render_model(&plan, &flow, &nodes_by_id);

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
                    orient="auto-start-reverse"
                >
                    <path d="M 0 0 L 10 5 L 0 10 z" class="flow-arrowhead"></path>
                </marker>
            </defs>
            <g class="flow-transform" transform=transform>
                // Z-order: edges, then labels, then decisions, then node cards on top.
                <g class="flow-edges">
                    {edge_items.into_iter().map(|e| view! { <DiagramEdge edge=e/> }).collect_view()}
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
