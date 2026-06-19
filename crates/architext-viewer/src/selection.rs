//! Selection logic — a thin adapter over the ported routing view-selection.
//!
//! The diagram-relevant rules (which views a mode shows, which flows a view can
//! render, default picks) live in
//! `architext_routing::plan_request::view_selection`. This module adapts the
//! viewer's richer `View`/`Flow` models to those functions and resolves results
//! back to viewer-side indices, so the viewer never re-implements the rules.

use architext_routing::plan_request::view_selection as vs;

use crate::data::models::{Flow, View};
use crate::theme::Mode;

/// Build the routing-type view list (parallel to `views` by index).
fn routing_views(views: &[View]) -> Vec<architext_routing::plan_request::types::View> {
    views.iter().map(View::to_routing).collect()
}

/// Build the routing-type flow list (parallel to `flows` by index).
fn routing_flows(flows: &[Flow]) -> Vec<architext_routing::plan_request::types::Flow> {
    flows.iter().map(Flow::to_routing).collect()
}

/// Index of the view whose id matches `id`.
fn view_index_by_id(views: &[View], id: &str) -> Option<usize> {
    views.iter().position(|v| v.id == id)
}

/// Index of the flow whose id matches `id`.
fn flow_index_by_id(flows: &[Flow], id: &str) -> Option<usize> {
    flows.iter().position(|f| f.id == id)
}

/// The indices of views that `mode` projects, in document order.
pub fn views_for_mode(views: &[View], mode: Mode) -> Vec<usize> {
    let types = vs::view_types_for_mode(mode.id());
    views
        .iter()
        .enumerate()
        .filter(|(_, v)| types.contains(&v.view_type.as_str()))
        .map(|(i, _)| i)
        .collect()
}

/// The default view index for a mode (the routing rule: first matching type,
/// else the document default / first view).
pub fn default_view_for_mode(views: &[View], mode: Mode) -> Option<usize> {
    let rv = routing_views(views);
    let fallback = rv.first();
    let chosen = vs::default_view_for_mode(mode.id(), &rv, fallback)?;
    view_index_by_id(views, &chosen.id)
}

/// Indices of flow-projection views compatible with the given flow.
pub fn compatible_flow_views(views: &[View], flows: &[Flow], flow_idx: usize) -> Vec<usize> {
    let rv = routing_views(views);
    let rf = flows[flow_idx].to_routing();
    vs::compatible_flow_views_for_flow(&rv, &rf)
        .into_iter()
        .filter_map(|v| view_index_by_id(views, &v.id))
        .collect()
}

/// Indices of flows compatible with the given view.
pub fn compatible_flows_for_view(views: &[View], flows: &[Flow], view_idx: usize) -> Vec<usize> {
    let rf = routing_flows(flows);
    let rv = views[view_idx].to_routing();
    vs::compatible_flows_for_view(&rf, &rv)
        .into_iter()
        .filter_map(|f| flow_index_by_id(flows, &f.id))
        .collect()
}

/// The default view index when a flow is selected (prefers an authored
/// projection over `system-map`; keeps a still-compatible current view).
pub fn default_view_for_flow(
    views: &[View],
    flows: &[Flow],
    mode: Mode,
    current_view: Option<usize>,
    flow_idx: usize,
) -> Option<usize> {
    let rv = routing_views(views);
    let rf = flows[flow_idx].to_routing();
    let current = current_view.and_then(|i| rv.get(i));
    let fallback = rv.first();
    let chosen = vs::default_view_for_flow(mode.id(), current, &rv, &rf, fallback)?;
    view_index_by_id(views, &chosen.id)
}

/// The child C4 view type for a parent C4 type, or `None` at the leaf.
/// Port of JS `childC4Type` in `c4Drilldown.js`.
fn child_c4_type(view_type: &str) -> Option<&'static str> {
    match view_type {
        "c4-context" => Some("c4-container"),
        "c4-container" => Some("c4-component"),
        "c4-component" => Some("c4-code"),
        _ => None,
    }
}

/// The index of the child C4 view a node drills down into, if one exists.
/// Port of JS `childC4ViewForNode(views, activeView, nodeId)`: find the view
/// whose type is the parent's child type AND whose `scopeNodeId == nodeId`.
///
/// Returns `None` when the active view is not a C4 view, is at the leaf level,
/// or no scoped child view is authored for the node — in which case the caller
/// falls back to selecting the node (inspector), as the JS viewer does.
pub fn child_c4_view_for_node(
    views: &[View],
    active_view_type: &str,
    node_id: &str,
) -> Option<usize> {
    if !active_view_type.starts_with("c4-") {
        return None;
    }
    let next_type = child_c4_type(active_view_type)?;
    views.iter().position(|v| {
        v.view_type == next_type && v.scope_node_id.as_deref() == Some(node_id)
    })
}

/// The set of node ids in `active_view_type` that DRILL DOWN — i.e. that have a
/// scoped child C4 view at the next level. Empty when the active view is not a
/// C4 view or is at the leaf level (no child type). Drives the per-card
/// drilldown affordance: a node in this set is decomposable; one absent from it
/// is a leaf/external node with no child view, so the affordance is suppressed.
pub fn drillable_node_ids(
    views: &[View],
    active_view_type: &str,
) -> std::collections::HashSet<String> {
    let Some(next_type) = child_c4_type(active_view_type) else {
        return std::collections::HashSet::new();
    };
    views
        .iter()
        .filter(|v| v.view_type == next_type)
        .filter_map(|v| v.scope_node_id.clone())
        .collect()
}

/// The default flow index when a view is selected (keeps the current flow if it
/// stays compatible; else the first compatible flow).
pub fn default_flow_for_view(
    views: &[View],
    flows: &[Flow],
    view_idx: usize,
    current_flow: Option<usize>,
) -> Option<usize> {
    let rf = routing_flows(flows);
    let rv = views[view_idx].to_routing();
    let current = current_flow.and_then(|i| rf.get(i));
    let fallback = rf.first();
    let chosen = vs::default_flow_for_view(&rv, current, &rf, fallback)?;
    flow_index_by_id(flows, &chosen.id)
}

#[cfg(test)]
mod c4_drilldown_tests {
    use super::*;

    /// A C4 hierarchy: Context (root, no scope) → Container scoped to `sys` →
    /// Component scoped to `svc`. `external` is a Context node with no child
    /// view (it must NOT be drillable / resolve a child).
    fn c4_views() -> Vec<View> {
        serde_json::from_value(serde_json::json!([
            { "id": "ctx", "name": "Context", "type": "c4-context", "lanes": [] },
            { "id": "cont", "name": "Container", "type": "c4-container", "scopeNodeId": "sys", "lanes": [] },
            { "id": "comp", "name": "Component", "type": "c4-component", "scopeNodeId": "svc", "lanes": [] }
        ]))
        .expect("valid views json")
    }

    #[test]
    fn node_with_scoped_child_drills_to_that_child() {
        // WHY: drilldown must follow the authored scopeNodeId anchor — clicking
        // `sys` in the Context view must open the Container view that scopes it,
        // never some other view at that level.
        let views = c4_views();
        let idx = child_c4_view_for_node(&views, "c4-context", "sys");
        assert_eq!(idx, Some(1), "Context node `sys` drills to the Container view");

        let idx = child_c4_view_for_node(&views, "c4-container", "svc");
        assert_eq!(idx, Some(2), "Container node `svc` drills to the Component view");
    }

    #[test]
    fn node_without_scoped_child_does_not_drill() {
        // WHY: a leaf/external node has no child view; the viewer must fall back
        // to inspecting it, never fabricate a drill target.
        let views = c4_views();
        assert_eq!(
            child_c4_view_for_node(&views, "c4-context", "external"),
            None,
            "a Context node with no scoped Container view does not drill",
        );
        // Leaf level: a c4-component view has child type c4-code; none authored.
        assert_eq!(
            child_c4_view_for_node(&views, "c4-component", "svc"),
            None,
            "no c4-code child authored → leaf, no drill",
        );
    }

    #[test]
    fn non_c4_view_never_drills() {
        // WHY: drilldown is C4-specific; a flow/system view must not resolve a
        // C4 child even if a node id happens to match a scopeNodeId.
        let views = c4_views();
        assert_eq!(child_c4_view_for_node(&views, "system-map", "sys"), None);
    }

    #[test]
    fn drillable_ids_are_exactly_the_scoped_children_at_the_next_level() {
        // WHY: the per-card affordance must light up exactly the decomposable
        // nodes (those with a scoped child at the NEXT level) and nothing else,
        // so the absence of the cue legibly means "no drilldown".
        let views = c4_views();
        let from_context = drillable_node_ids(&views, "c4-context");
        assert_eq!(from_context.len(), 1);
        assert!(from_context.contains("sys"), "`sys` decomposes into Container");
        assert!(!from_context.contains("external"), "external is not drillable");

        // At the leaf level there is no child type → nothing is drillable.
        assert!(drillable_node_ids(&views, "c4-component").is_empty());
        // Non-C4 views are never drillable.
        assert!(drillable_node_ids(&views, "system-map").is_empty());
    }
}
