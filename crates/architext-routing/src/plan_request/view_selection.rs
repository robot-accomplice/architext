//! Port of `viewer/src/presentation/viewSelection.js`.
//!
//! Two consumers share this module:
//! - the precompute farm (`enumerate_flow_plan_requests`) needs
//!   `flow_view_types` + `flow_compatible_with_view`;
//! - the wasm viewer needs the full mode/view/flow selection surface
//!   (`view_types_for_mode`, `default_view_for_mode`,
//!   `compatible_flow_views_for_flow`, `default_view_for_flow`,
//!   `default_flow_for_view`, `mode_for_view`).
//!
//! All functions operate on the routing `View`/`Flow` types so the logic lives
//! in exactly one place (DRY); the viewer adapts its richer models to these.

use crate::plan_request::types::{Flow, View};

/// The "flows" projection view types, in priority order.
/// Subset of `MODE_DEFINITIONS["flows"].viewTypes` and the body of
/// `viewTypesForMode("flows")`.
pub fn flow_view_types() -> &'static [&'static str] {
    &["system-map", "flow-explorer", "workflow", "dataflow"]
}

/// The view types each mode projects, keyed by the JS mode id.
/// Port of `MODE_DEFINITIONS[*].viewTypes` via `viewTypesForMode`.
///
/// Modes with no view types (`repo-tree`, `blast-radius`, `release-truth`,
/// `rules`) return an empty slice; they render non-diagram surfaces.
pub fn view_types_for_mode(mode: &str) -> &'static [&'static str] {
    match mode {
        "flows" => flow_view_types(),
        "sequence" => &["sequence"],
        "c4" => &["c4-context", "c4-container", "c4-component", "c4-code"],
        "deployment" => &["deployment"],
        "data-risks" => &["risk-overlay", "dataflow"],
        _ => &[],
    }
}

/// Port of JS `modeForView(view)`: the mode a view belongs to by its type.
pub fn mode_for_view(view: Option<&View>) -> &'static str {
    let Some(view) = view else { return "flows" };
    match view.view_type.as_str() {
        "sequence" => "sequence",
        "deployment" => "deployment",
        "risk-overlay" => "data-risks",
        t if t.starts_with("c4-") => "c4",
        _ => "flows",
    }
}

/// Port of JS `defaultViewForMode(mode, views, fallback)`.
///
/// Returns the first view whose type matches the mode's projection types, or
/// the fallback. Diagram-less modes (release-truth/rules/repo-tree/blast-radius)
/// always return the fallback.
pub fn default_view_for_mode<'a>(
    mode: &str,
    views: &'a [View],
    fallback: Option<&'a View>,
) -> Option<&'a View> {
    let types = view_types_for_mode(mode);
    if types.is_empty() {
        return fallback;
    }
    views
        .iter()
        .find(|v| types.contains(&v.view_type.as_str()))
        .or(fallback)
}

/// All endpoint node IDs referenced by a flow's steps (union of from + to).
/// Port of JS `flowEndpointIds`.
fn flow_endpoint_ids(flow: &Flow) -> std::collections::HashSet<&str> {
    flow.steps.iter().flat_map(|s| [s.from.as_str(), s.to.as_str()]).collect()
}

/// All node IDs present in a view across all lanes.
/// Port of JS `viewNodeIds`.
fn view_node_ids(view: &View) -> std::collections::HashSet<&str> {
    view.lanes.iter().flat_map(|l| l.node_ids.iter().map(|s| s.as_str())).collect()
}

/// Returns true if every endpoint of `flow` appears in at least one lane of `view`.
/// Port of JS `flowCompatibleWithView`.
pub fn flow_compatible_with_view(flow: &Flow, view: &View) -> bool {
    let endpoints = flow_endpoint_ids(flow);
    if endpoints.is_empty() {
        return true;
    }
    let nodes = view_node_ids(view);
    endpoints.iter().all(|id| nodes.contains(*id))
}

/// Views in the "flows" projection that are compatible with `flow`.
/// Port of JS `compatibleFlowViewsForFlow(views, flow)`.
pub fn compatible_flow_views_for_flow<'a>(views: &'a [View], flow: &Flow) -> Vec<&'a View> {
    let projection = flow_view_types();
    views
        .iter()
        .filter(|v| projection.contains(&v.view_type.as_str()) && flow_compatible_with_view(flow, v))
        .collect()
}

/// Flows compatible with `view`.
/// Port of JS `compatibleFlowsForView(flows, view)`.
pub fn compatible_flows_for_view<'a>(flows: &'a [Flow], view: &View) -> Vec<&'a Flow> {
    flows.iter().filter(|f| flow_compatible_with_view(f, view)).collect()
}

/// Port of JS `defaultViewForFlow(mode, currentView, views, flow, fallback)`.
///
/// In non-flows modes, keep the current view (or fall back to the mode default).
/// In flows mode, prefer an authored (non-`system-map`) compatible projection
/// over `system-map`, otherwise keep a still-compatible current view.
pub fn default_view_for_flow<'a>(
    mode: &str,
    current_view: Option<&'a View>,
    views: &'a [View],
    flow: &Flow,
    fallback: Option<&'a View>,
) -> Option<&'a View> {
    if mode != "flows" {
        return current_view.or_else(|| default_view_for_mode(mode, views, fallback));
    }
    let compatible = compatible_flow_views_for_flow(views, flow);
    let authored = compatible.iter().find(|v| v.view_type != "system-map").copied();
    if let Some(cv) = current_view {
        if cv.view_type == "system-map" {
            if let Some(a) = authored {
                return Some(a);
            }
        }
        if flow_compatible_with_view(flow, cv) {
            return Some(cv);
        }
    }
    authored
        .or_else(|| compatible.first().copied())
        .or_else(|| default_view_for_mode(mode, views, fallback))
}

/// Port of JS `defaultFlowForView(view, currentFlow, flows, fallback)`.
///
/// Keep the current flow if it stays compatible with `view`, else pick the
/// first compatible flow, else the fallback.
pub fn default_flow_for_view<'a>(
    view: &View,
    current_flow: Option<&'a Flow>,
    flows: &'a [Flow],
    fallback: Option<&'a Flow>,
) -> Option<&'a Flow> {
    if let Some(cf) = current_flow {
        if flow_compatible_with_view(cf, view) {
            return Some(cf);
        }
    }
    compatible_flows_for_view(flows, view).first().copied().or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_request::types::{Flow, FlowStep, Lane, View};

    fn mkflow(id: &str, steps: Vec<(&str, &str)>) -> Flow {
        Flow {
            id: id.to_string(),
            steps: steps.into_iter().enumerate().map(|(i, (from, to))| FlowStep {
                id: format!("s{i}"),
                from: from.to_string(),
                to: to.to_string(),
                action: "x".to_string(),
                summary: None,
                kind: None,
                outcome: None,
                return_of: None,
            }).collect(),
        }
    }

    fn mkview(id: &str, view_type: &str, lanes: Vec<Vec<&str>>) -> View {
        View {
            id: id.to_string(),
            view_type: view_type.to_string(),
            lanes: lanes.into_iter().enumerate().map(|(i, ids)| Lane {
                id: format!("l{i}"),
                node_ids: ids.into_iter().map(|s| s.to_string()).collect(),
            }).collect(),
        }
    }

    #[test]
    fn compatible_when_all_endpoints_in_view() {
        let flow = mkflow("f1", vec![("a", "b"), ("b", "c")]);
        let view = mkview("v1", "system-map", vec![vec!["a", "b", "c"]]);
        assert!(flow_compatible_with_view(&flow, &view));
    }

    #[test]
    fn incompatible_when_missing_endpoint() {
        let flow = mkflow("f1", vec![("a", "b"), ("b", "c")]);
        let view = mkview("v1", "system-map", vec![vec!["a", "b"]]); // "c" missing
        assert!(!flow_compatible_with_view(&flow, &view));
    }

    #[test]
    fn flow_view_types_includes_expected() {
        let types = flow_view_types();
        assert!(types.contains(&"system-map"));
        assert!(types.contains(&"workflow"));
        assert!(types.contains(&"dataflow"));
        assert!(types.contains(&"flow-explorer"));
    }

    #[test]
    fn view_types_for_mode_matches_definitions() {
        assert_eq!(view_types_for_mode("sequence"), &["sequence"]);
        assert_eq!(view_types_for_mode("deployment"), &["deployment"]);
        assert_eq!(view_types_for_mode("c4").len(), 4);
        assert_eq!(view_types_for_mode("data-risks"), &["risk-overlay", "dataflow"]);
        assert!(view_types_for_mode("rules").is_empty());
        assert!(view_types_for_mode("release-truth").is_empty());
    }

    #[test]
    fn mode_for_view_classifies_types() {
        assert_eq!(mode_for_view(Some(&mkview("v", "sequence", vec![]))), "sequence");
        assert_eq!(mode_for_view(Some(&mkview("v", "c4-container", vec![]))), "c4");
        assert_eq!(mode_for_view(Some(&mkview("v", "risk-overlay", vec![]))), "data-risks");
        assert_eq!(mode_for_view(Some(&mkview("v", "system-map", vec![]))), "flows");
        assert_eq!(mode_for_view(None), "flows");
    }

    #[test]
    fn default_view_for_mode_picks_matching_type() {
        let views = vec![
            mkview("sm", "system-map", vec![]),
            mkview("seq", "sequence", vec![]),
        ];
        let chosen = default_view_for_mode("sequence", &views, views.first());
        assert_eq!(chosen.unwrap().id, "seq");
        // diagram-less mode returns fallback
        let fb = default_view_for_mode("rules", &views, views.first());
        assert_eq!(fb.unwrap().id, "sm");
    }

    #[test]
    fn default_view_for_flow_prefers_authored_over_system_map() {
        let views = vec![
            mkview("sm", "system-map", vec![vec!["a", "b"]]),
            mkview("we", "workflow", vec![vec!["a", "b"]]),
        ];
        let flow = mkflow("f1", vec![("a", "b")]);
        let current = views.first(); // currently system-map
        let chosen = default_view_for_flow("flows", current, &views, &flow, views.first());
        assert_eq!(chosen.unwrap().id, "we");
    }

    #[test]
    fn default_flow_for_view_keeps_compatible_current() {
        let view = mkview("sm", "system-map", vec![vec!["a", "b", "c"]]);
        let flows = vec![mkflow("f1", vec![("a", "b")]), mkflow("f2", vec![("b", "c")])];
        let current = flows.first();
        let chosen = default_flow_for_view(&view, current, &flows, flows.first());
        assert_eq!(chosen.unwrap().id, "f1");
    }
}
