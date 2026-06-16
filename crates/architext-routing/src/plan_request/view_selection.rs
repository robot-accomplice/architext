//! Port of the farm-relevant subset of `viewer/src/presentation/viewSelection.js`.
//!
//! Only the functions needed by `enumerateFlowPlanRequests`:
//! - `view_types_for_mode("flows")`
//! - `flow_compatible_with_view`

use crate::plan_request::types::{Flow, View};

/// View types that belong to the "flows" mode.
/// Port of the `MODE_DEFINITIONS` flows entry and `viewTypesForMode`.
pub fn flow_view_types() -> &'static [&'static str] {
    &["system-map", "flow-explorer", "workflow", "dataflow"]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_request::types::{Flow, FlowStep, Lane, View};

    fn mkflow(steps: Vec<(&str, &str)>) -> Flow {
        Flow {
            id: "f1".to_string(),
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

    fn mkview(lanes: Vec<Vec<&str>>) -> View {
        View {
            id: "v1".to_string(),
            view_type: "system-map".to_string(),
            lanes: lanes.into_iter().enumerate().map(|(i, ids)| Lane {
                id: format!("l{i}"),
                node_ids: ids.into_iter().map(|s| s.to_string()).collect(),
            }).collect(),
        }
    }

    #[test]
    fn compatible_when_all_endpoints_in_view() {
        let flow = mkflow(vec![("a", "b"), ("b", "c")]);
        let view = mkview(vec![vec!["a", "b", "c"]]);
        assert!(flow_compatible_with_view(&flow, &view));
    }

    #[test]
    fn incompatible_when_missing_endpoint() {
        let flow = mkflow(vec![("a", "b"), ("b", "c")]);
        let view = mkview(vec![vec!["a", "b"]]); // "c" missing
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
}
