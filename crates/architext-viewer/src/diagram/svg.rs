//! The diagram `<svg>`: fluid viewBox, pan/zoom transform group, marker defs,
//! and composition of the node/edge/label/decision renderers.
//!
//! DESIGN.md rule 3: the SVG is FLUID — `width=100%`/`height=100%` with a
//! `viewBox` sized to the plan's canvas. It NEVER has fixed pixel w/h. Pan/zoom
//! are applied as a single `transform` on the inner `<g>` (translate + scale),
//! driven by signals owned by the canvas panel.

use std::collections::{HashMap, HashSet};

use architext_routing::model::{Plan, Point, Rect};
use architext_routing::plan_request::DECISION_STEM_PREFIX;
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
/// A decision diamond ready to render: its rect, the host component's role color
/// (tint), and the annotation — the decision step's `action`, i.e. WHAT is being
/// decided (viewer/DESIGN.md "Flow decision diamonds").
#[derive(Clone)]
pub struct DecisionView {
    pub rect: Rect,
    pub role_var: String,
    pub annotation: Option<String>,
}

/// A decision branch's outcome label (e.g. `valid` / `invalid`): the text the
/// branch represents, anchored just outside the diamond on that branch line.
/// `step_id` is the branch step so it highlights when that step is selected.
#[derive(Clone)]
pub struct OutcomeLabel {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub step_id: String,
}

#[derive(Clone)]
pub struct RenderModel {
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub nodes: Vec<NodeView>,
    pub decisions: Vec<DecisionView>,
    pub outcome_labels: Vec<OutcomeLabel>,
    pub edges: Vec<EdgeView>,
    pub labels: Vec<LabelView>,
    /// Top-left anchor of the "NOT IN THIS FLOW" overline above the parked
    /// out-of-flow cluster, in canvas/plan coordinates. `None` when there are no
    /// out-of-flow cards (structural modes, or a flow that covers every node).
    pub parked_label: Option<(f64, f64)>,
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
    drillable_ids: &HashSet<String>,
) -> RenderModel {
    // Edge kind per step id (the route key == the flow step id). Empty in
    // structural mode → every edge defaults to Process.
    let kind_by_step: HashMap<&str, EdgeKind> = flow
        .map(|f| f.steps.iter()
            .map(|s| (s.id.as_str(), EdgeKind::from_step_kind(s.kind.as_deref())))
            .collect())
        .unwrap_or_default();

    // The "in active flow" id set: the union of every flow step's `from` + `to`
    // endpoints. `None` flow (structural C4/Deployment) → no orphan concept, so
    // `is_in_flow` treats every node as in-flow.
    let in_flow_ids = flow_node_ids(flow);
    let is_in_flow = |id: &str| flow.is_none() || in_flow_ids.contains(id);

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
                in_flow: is_in_flow(id),
                scale: 1.0,
                drillable: drillable_ids.contains(id),
            }),
            None => {
                // An augmented (decision) rect — tint with the component's role
                // color when the affiliated node is resolvable, else external.
                // Structural mode has no decision rects (no flow), but guard
                // anyway: default to the external role when there is no flow.
                let component_type = flow
                    .map(|f| decision_component_type(id, f, nodes_by_id))
                    .unwrap_or("external");
                // The annotation — what is being decided — is the decision step's
                // `action`, looked up from the flow by the diamond's step id.
                let annotation = flow.and_then(|f| decision_annotation(id, f));
                decisions.push(DecisionView {
                    rect: rect.clone(),
                    role_var: role_color_var(component_type),
                    annotation,
                });
            }
        }
    }

    // Branch outcome labels (e.g. `valid` / `invalid`): one per flow step that
    // carries an `outcome`, anchored just outside the diamond on that branch.
    let outcome_labels = flow
        .map(|f| build_outcome_labels(f, plan))
        .unwrap_or_default();

    // Edges: the `d`-string verbatim, with the resolved kind. A decision-stem
    // route (`decision-stem:<step>`) is not a flow step, so it is not in
    // `kind_by_step`; detect it by its id prefix and render it as a Stem (no
    // arrowhead).
    let edges = plan
        .routes
        .iter()
        .map(|(id, route)| EdgeView {
            id: id.clone(),
            d: route.d.clone(),
            kind: if id.starts_with(DECISION_STEM_PREFIX) {
                EdgeKind::Stem
            } else {
                kind_by_step.get(id.as_str()).copied().unwrap_or(EdgeKind::Process)
            },
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
        // for the hover title + legend). A number badge carries its route id (==
        // step id) so it can highlight when that step is selected.
        let (kind, step_id) = if collapsed != raw_label {
            (LabelKind::Number(collapsed), Some(id.clone()))
        } else {
            (
                LabelKind::Relationship {
                    kind: RelationshipKind::classify(&raw_label),
                    word: raw_label,
                },
                None,
            )
        };
        let box_rect = plan.label_boxes.get(id).cloned().unwrap_or(Rect {
            x: route.label_x,
            y: route.label_y,
            width: 0.0,
            height: 0.0,
        });
        labels.push(LabelView {
            kind,
            step_id,
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

    // Park the out-of-flow ("unrelated") cards into a compact, dimmed cluster to
    // the RIGHT of the in-flow bounding box (UX #2 — they're visible, secondary,
    // not hidden). The engine interleaves them with the flow, so we override
    // their rects in the viewer render layer (the engine is untouched).
    let parked_label = park_unrelated(&mut node_views);

    RenderModel {
        canvas_width: plan.canvas_width,
        canvas_height: plan.canvas_height,
        nodes: node_views,
        decisions,
        outcome_labels,
        edges,
        labels,
        parked_label,
    }
}

/// Geometry of the parked out-of-flow cluster (canvas px). Cards render
/// `PARK_SCALE`× the full card; `PARK_GAP_X` separates the in-flow bbox from the
/// cluster, `PARK_GAP` is the in-cluster gutter, and the column wraps to
/// `PARK_COLS` once it exceeds `PARK_MAX_ROWS` cards (a 2-col grid for many
/// orphans, like the demo's tall column kept compact).
const PARK_SCALE: f64 = 0.62;
const PARK_GAP_X: f64 = 64.0;
const PARK_GAP: f64 = 10.0;
const PARK_MAX_ROWS: usize = 12;
const PARK_COLS: usize = 2;
/// Vertical room reserved above the first parked card for the "NOT IN THIS FLOW"
/// overline (so the label clears the top card).
const PARK_LABEL_GAP: f64 = 18.0;

/// The placed slot (top-left + scaled footprint, in canvas px) of one parked
/// out-of-flow card, plus the "NOT IN THIS FLOW" overline anchor for the cluster.
pub struct ParkLayout {
    /// Parked-card slots in iteration order of the input ids: top-left of the
    /// rendered (already-scaled) footprint and its `width`×`height`.
    pub slots: Vec<Rect>,
    /// Top-left anchor of the cluster overline (above the first parked card).
    pub label_anchor: (f64, f64),
}

/// Compute the parked-cluster layout to the RIGHT of the in-flow bounding box.
///
/// Shared by the render model (which moves each out-of-flow `NodeView` into its
/// slot) and `content_bounds`/fit (which must frame the parked cluster at its
/// repositioned location, not the engine's interleaved one). The engine places
/// out-of-flow nodes interleaved with the flow, so this is a VIEWER render-layer
/// transform — the engine is untouched.
///
/// `in_flow_rects` are the engine rects of the in-flow cards (the frame the
/// cluster sits beside); `parked_natural` are the engine rects of the
/// out-of-flow cards, in the order their slots are returned. Cards lay out in a
/// single column, wrapping to [`PARK_COLS`] columns past [`PARK_MAX_ROWS`], each
/// at [`PARK_SCALE`]. Returns `None` when either set is empty.
pub fn park_layout(in_flow_rects: &[Rect], parked_natural: &[Rect]) -> Option<ParkLayout> {
    if in_flow_rects.is_empty() || parked_natural.is_empty() {
        return None;
    }
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for r in in_flow_rects {
        min_y = min_y.min(r.y);
        max_x = max_x.max(r.x + r.width);
    }

    // Rendered cell size: natural card size scaled down. All engine node rects
    // share the configured node size, so the first is representative.
    let cell_w = parked_natural[0].width * PARK_SCALE;
    let cell_h = parked_natural[0].height * PARK_SCALE;
    let cols = if parked_natural.len() > PARK_MAX_ROWS { PARK_COLS } else { 1 };
    let origin_x = max_x + PARK_GAP_X;
    let origin_y = min_y + PARK_LABEL_GAP;

    let slots = (0..parked_natural.len())
        .map(|slot| {
            let col = slot % cols;
            let row = slot / cols;
            Rect {
                x: origin_x + col as f64 * (cell_w + PARK_GAP),
                y: origin_y + row as f64 * (cell_h + PARK_GAP),
                width: cell_w,
                height: cell_h,
            }
        })
        .collect();

    Some(ParkLayout { slots, label_anchor: (origin_x, min_y) })
}

/// Reposition every out-of-flow `NodeView` into the parked cluster (via
/// [`park_layout`]) and return the cluster overline anchor. The card keeps its
/// NATURAL `width`/`height` and gets `scale = PARK_SCALE`; only `x`/`y` move to
/// the slot top-left (the card scales uniformly at render time). Returns `None`
/// when there is nothing to park.
fn park_unrelated(nodes: &mut [NodeView]) -> Option<(f64, f64)> {
    let in_flow_rects: Vec<Rect> =
        nodes.iter().filter(|n| n.in_flow).map(|n| n.rect.clone()).collect();
    let parked_idx: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.in_flow)
        .map(|(i, _)| i)
        .collect();
    let parked_natural: Vec<Rect> =
        parked_idx.iter().map(|&i| nodes[i].rect.clone()).collect();

    let layout = park_layout(&in_flow_rects, &parked_natural)?;
    for (&idx, slot) in parked_idx.iter().zip(&layout.slots) {
        nodes[idx].scale = PARK_SCALE;
        nodes[idx].rect.x = slot.x;
        nodes[idx].rect.y = slot.y;
    }
    Some(layout.label_anchor)
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

/// The set of node ids that participate in the active flow: the union of every
/// flow step's `from` and `to` endpoints. `None` flow (structural C4/Deployment
/// mode) returns an empty set — callers treat "no flow" as "every node in flow".
pub fn flow_node_ids(flow: Option<&Flow>) -> HashSet<String> {
    let mut ids = HashSet::new();
    if let Some(f) = flow {
        for step in &f.steps {
            ids.insert(step.from.clone());
            ids.insert(step.to.clone());
        }
    }
    ids
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

/// The decision diamond's annotation — WHAT is being decided. This is the
/// `action` of the `decision`-kind step the diamond was built from
/// (`decision:<stepId>` → step.action), e.g. `"validateStarterModel"`. The full
/// exposition still lives in the steps panel; this is the on-diagram caption.
fn decision_annotation(decision_id: &str, flow: &Flow) -> Option<String> {
    let step_id = decision_id.strip_prefix("decision:").unwrap_or(decision_id);
    flow.steps
        .iter()
        .find(|s| s.id == step_id)
        .map(|s| s.action.clone())
}

/// Distance (canvas px) from a branch's diamond-tip start at which to anchor its
/// outcome label, measured ALONG the branch's first segment so the label sits
/// just outside the diamond on the line it belongs to.
const OUTCOME_LABEL_OFFSET: f64 = 18.0;

/// Build the branch outcome labels: one per flow step carrying an `outcome`,
/// anchored `OUTCOME_LABEL_OFFSET` px along that branch's route from the diamond
/// tip. Steps with no routed edge (or a degenerate route) are skipped.
fn build_outcome_labels(flow: &Flow, plan: &Plan) -> Vec<OutcomeLabel> {
    flow.steps
        .iter()
        .filter_map(|s| {
            let outcome = s.outcome.as_ref()?;
            let route = plan.routes.get(&s.id)?;
            let (x, y) = point_along(&route.points, OUTCOME_LABEL_OFFSET)?;
            // Highlight with the DECISION step, not the branch: a branch shares
            // its decision's selection (and the steps panel collapses branches
            // under it), so selecting the decision lights the whole cluster —
            // stem + every outcome label — together (viewer/DESIGN.md).
            let decision_step_id = flow
                .steps
                .iter()
                .find(|d| d.kind.as_deref() == Some("decision") && d.to == s.from)
                .map(|d| d.id.clone())
                .unwrap_or_else(|| s.id.clone());
            Some(OutcomeLabel { x, y, text: outcome.clone(), step_id: decision_step_id })
        })
        .collect()
}

/// The point `dist` canvas-px along a polyline from its first vertex. Returns the
/// last vertex if `dist` runs past the end, and `None` for a polyline with fewer
/// than two points.
fn point_along(points: &[Point], dist: f64) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let mut remaining = dist;
    for seg in points.windows(2) {
        let (a, b) = (&seg[0], &seg[1]);
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        if len >= remaining {
            let t = if len == 0.0 { 0.0 } else { remaining / len };
            return Some((a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
        remaining -= len;
    }
    points.last().map(|p| (p.x, p.y))
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
    /// Node ids whose card DRILLS DOWN to a scoped C4 child view (C4 mode only;
    /// empty in every other mode). Drives the per-card drilldown affordance. The
    /// caller computes this from the active C4 view + all views, so the SVG layer
    /// stays agnostic of the C4 hierarchy rules.
    #[prop(optional)] drillable_node_ids: HashSet<String>,
    #[prop(into)] pan_x: Signal<f64>,
    #[prop(into)] pan_y: Signal<f64>,
    #[prop(into)] zoom: Signal<f64>,
    #[prop(into)] selected_node: Signal<Option<String>>,
    /// The selected flow step id (steps-panel selection). The flow route id
    /// equals the step id, so an edge whose id matches gets the `--accent` active
    /// treatment. Always `None` in structural (C4 / deployment) mode.
    #[prop(into)] selected_step: Signal<Option<String>>,
    /// Whether to render the parked out-of-flow ("unrelated") node cards (dimmed,
    /// smaller, clustered to the right) and their overline. `true` (the default)
    /// keeps them VISIBLE but secondary so the active flow dominates; toggling
    /// `false` fully hides them to declutter. In structural (C4 / deployment)
    /// mode there is no flow, every node is in-flow, and this prop has no effect.
    #[prop(into)] show_unrelated: Signal<bool>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let nodes_by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let model =
        build_render_model(&plan, flow.as_ref(), &edge_labels, &nodes_by_id, &drillable_node_ids);

    let view_box = format!("0 0 {} {}", model.canvas_width, model.canvas_height);
    let transform = move || format!("translate({} {}) scale({})", pan_x.get(), pan_y.get(), zoom.get());

    let node_items = model.nodes.clone();
    let decision_items = model.decisions.clone();
    let outcome_items = model.outcome_labels.clone();
    let edge_items = model.edges.clone();
    let label_items = model.labels.clone();
    let parked_label = model.parked_label;

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
                        // A stem's route id is `decision-stem:<step>`; it should
                        // light up with its decision STEP, so match on the
                        // stripped step id. Other edges match their route id
                        // (== step id) directly.
                        let match_id = e.id
                            .strip_prefix(DECISION_STEM_PREFIX)
                            .unwrap_or(&e.id)
                            .to_string();
                        let is_selected = Signal::derive(move || {
                            selected_step.get().as_deref() == Some(match_id.as_str())
                        });
                        view! { <DiagramEdge edge=e selected=is_selected/> }
                    }).collect_view()}
                </g>
                <g class="flow-labels">
                    {label_items.into_iter().map(|l| {
                        let sid = l.step_id.clone();
                        let is_selected = Signal::derive(move || {
                            sid.is_some() && selected_step.get().as_deref() == sid.as_deref()
                        });
                        view! { <DiagramLabel label=l selected=is_selected/> }
                    }).collect_view()}
                </g>
                <g class="flow-decisions">
                    {decision_items.into_iter().map(|d| view! {
                        <DecisionDiamond rect=d.rect role_var=d.role_var annotation=d.annotation/>
                    }).collect_view()}
                </g>
                // Branch outcome labels (e.g. `valid` / `invalid`), each anchored
                // just outside the diamond on its branch line. Lights up with the
                // `--accent` STATE when its branch step is selected.
                <g class="flow-outcomes">
                    {outcome_items.into_iter().map(|o| {
                        let sid = o.step_id.clone();
                        let is_selected = Signal::derive(move || {
                            selected_step.get().as_deref() == Some(sid.as_str())
                        });
                        view! {
                            <text
                                class="flow-outcome"
                                class=("flow-outcome--active", move || is_selected.get())
                                x=o.x
                                y=o.y
                            >
                                {o.text}
                            </text>
                        }
                    }).collect_view()}
                </g>
                // "NOT IN THIS FLOW" overline above the parked cluster. Drawn
                // with the parked cards (same `show_unrelated` gate); absent when
                // there is nothing parked.
                {parked_label.map(|(lx, ly)| view! {
                    <text
                        class="flow-parked-label overline"
                        x=lx
                        y=ly
                        style=move || if show_unrelated.get() { "" } else { "display:none" }
                    >
                        "NOT IN THIS FLOW"
                    </text>
                })}
                <g class="flow-nodes">
                    {move || {
                        // Parked out-of-flow cards are VISIBLE by default; the
                        // toggle (`show_unrelated`) lets the user fully hide them
                        // to declutter.
                        let show = show_unrelated.get();
                        node_items
                            .iter()
                            .filter(|n| n.in_flow || show)
                            .cloned()
                            .map(|n| {
                                let id = n.id.clone();
                                let is_selected = Signal::derive(move || {
                                    selected_node.get().as_deref() == Some(id.as_str())
                                });
                                view! { <DiagramNode node=n selected=is_selected on_select=on_select/> }
                            })
                            .collect_view()
                    }}
                </g>
            </g>
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::{flow_node_ids, park_layout, pill_label, PARK_SCALE};
    use architext_routing::model::Rect;
    use crate::data::models::{Flow, FlowStep};

    fn rect(x: f64, y: f64) -> Rect {
        Rect { x, y, width: 136.0, height: 54.0 }
    }

    #[test]
    fn park_layout_places_cluster_right_of_in_flow_bbox() {
        // In-flow bbox spans x 100..436; the cluster must start to the right of
        // the in-flow max-x, never overlapping the flow.
        let in_flow = [rect(100.0, 100.0), rect(300.0, 200.0)];
        let parked = [rect(0.0, 0.0), rect(0.0, 0.0)];
        let layout = park_layout(&in_flow, &parked).expect("layout");
        let in_flow_max_x = 300.0 + 136.0;
        for slot in &layout.slots {
            assert!(slot.x > in_flow_max_x, "slot.x={} must clear flow", slot.x);
        }
        assert!(layout.label_anchor.0 > in_flow_max_x, "label clears flow");
    }

    #[test]
    fn park_layout_cards_are_smaller_than_natural() {
        let in_flow = [rect(0.0, 0.0)];
        let parked = [rect(0.0, 0.0)];
        let layout = park_layout(&in_flow, &parked).expect("layout");
        let s = &layout.slots[0];
        assert!((s.width - 136.0 * PARK_SCALE).abs() < 1e-9, "scaled width");
        assert!((s.height - 54.0 * PARK_SCALE).abs() < 1e-9, "scaled height");
        assert!(s.width < 136.0 && s.height < 54.0, "parked card is smaller");
    }

    #[test]
    fn park_layout_single_column_until_threshold_then_two() {
        let in_flow = [rect(0.0, 0.0)];
        // 12 parked → single column (all share one x).
        let twelve: Vec<Rect> = (0..12).map(|_| rect(0.0, 0.0)).collect();
        let l = park_layout(&in_flow, &twelve).expect("layout");
        assert!(l.slots.iter().all(|s| (s.x - l.slots[0].x).abs() < 1e-9), "1 col");
        // 13 parked → wraps to two columns (two distinct x values).
        let thirteen: Vec<Rect> = (0..13).map(|_| rect(0.0, 0.0)).collect();
        let l2 = park_layout(&in_flow, &thirteen).expect("layout");
        let xs: std::collections::HashSet<u64> =
            l2.slots.iter().map(|s| s.x.to_bits()).collect();
        assert_eq!(xs.len(), 2, "should wrap to 2 columns");
    }

    #[test]
    fn park_layout_none_when_no_in_flow_or_no_parked() {
        assert!(park_layout(&[], &[rect(0.0, 0.0)]).is_none(), "no in-flow");
        assert!(park_layout(&[rect(0.0, 0.0)], &[]).is_none(), "nothing to park");
    }

    fn step(id: &str, from: &str, to: &str) -> FlowStep {
        FlowStep {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            action: String::new(),
            summary: None,
            kind: None,
            outcome: None,
            return_of: None,
        }
    }

    #[test]
    fn flow_node_ids_is_union_of_step_endpoints() {
        let flow = Flow {
            id: "f".into(),
            name: "f".into(),
            status: None,
            summary: None,
            trigger: None,
            steps: vec![step("s1", "a", "b"), step("s2", "b", "c")],
            sequence_frames: vec![],
        };
        let ids = flow_node_ids(Some(&flow));
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("a") && ids.contains("b") && ids.contains("c"));
    }

    #[test]
    fn flow_node_ids_empty_without_flow() {
        // Structural (C4/Deployment) mode → no flow → empty set, and callers
        // treat that as "every node is in-flow".
        assert!(flow_node_ids(None).is_empty());
    }

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
