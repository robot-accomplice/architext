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
