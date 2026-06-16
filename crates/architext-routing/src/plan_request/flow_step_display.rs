//! Port of `viewer/src/presentation/flowStepDisplayModel.js`.
//!
//! Computes per-step display indices for a flow's steps, accounting for
//! decision branches that share the display index of their decision step.

use std::collections::HashMap;
use crate::plan_request::types::FlowStep;

/// Port of JS `flowStepDisplayIndexes(steps)`.
///
/// Returns a map from step id → display index (1-based).
pub fn flow_step_display_indexes(steps: &[FlowStep]) -> HashMap<String, usize> {
    let mut display_indexes: HashMap<String, usize> = HashMap::new();
    // Map from node id → the last decision step's 1-based index that targets that node.
    let mut latest_decision_index_by_node: HashMap<String, usize> = HashMap::new();

    for (index, step) in steps.iter().enumerate() {
        let decision_index = if step.outcome.is_some() {
            latest_decision_index_by_node.get(&step.from).copied()
        } else {
            None
        };
        let di = decision_index.unwrap_or(index + 1);
        display_indexes.insert(step.id.clone(), di);
        if step.kind.as_deref() == Some("decision") {
            latest_decision_index_by_node.insert(step.to.clone(), index + 1);
        }
    }
    display_indexes
}

/// Port of JS `decisionBranchTargets(steps)`.
///
/// Returns the set of node ids that are both:
/// - the `to` target of a decision step, and
/// - the `from` source of at least one outcome step.
pub fn decision_branch_targets(steps: &[FlowStep]) -> std::collections::HashSet<String> {
    // Map from node_id → decision step (only the last one wins, matching JS Map semantics)
    let mut targets: HashMap<String, &FlowStep> = HashMap::new();
    for step in steps {
        if step.kind.as_deref() == Some("decision") {
            targets.insert(step.to.clone(), step);
        }
    }
    let mut branched: std::collections::HashSet<String> = std::collections::HashSet::new();
    for step in steps {
        if step.outcome.is_some() && targets.contains_key(&step.from) {
            branched.insert(step.from.clone());
        }
    }
    branched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_request::types::FlowStep;

    fn step(id: &str, from: &str, to: &str, kind: Option<&str>, outcome: Option<&str>) -> FlowStep {
        FlowStep {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            action: "do".to_string(),
            summary: None,
            kind: kind.map(|s| s.to_string()),
            outcome: outcome.map(|s| s.to_string()),
            return_of: None,
        }
    }

    #[test]
    fn simple_steps_are_one_based() {
        let steps = vec![
            step("s1", "a", "b", None, None),
            step("s2", "b", "c", None, None),
        ];
        let idx = flow_step_display_indexes(&steps);
        assert_eq!(idx["s1"], 1);
        assert_eq!(idx["s2"], 2);
    }

    #[test]
    fn decision_branch_inherits_decision_index() {
        // decision step at index 1 (display=2); outcome branch at index 2 (display should be 2 too)
        let steps = vec![
            step("s1", "a", "b", None, None),
            step("s2", "b", "c", Some("decision"), None),  // decision: b→c, display=2
            step("s3", "c", "d", None, Some("yes")),        // outcome from c → display=2
            step("s4", "c", "e", None, Some("no")),         // outcome from c → display=2
        ];
        let idx = flow_step_display_indexes(&steps);
        assert_eq!(idx["s1"], 1);
        assert_eq!(idx["s2"], 2);
        assert_eq!(idx["s3"], 2);
        assert_eq!(idx["s4"], 2);
    }

    #[test]
    fn decision_branch_targets_set() {
        let steps = vec![
            step("s1", "a", "b", Some("decision"), None),
            step("s2", "b", "c", None, Some("yes")),
            step("s3", "b", "d", None, Some("no")),
        ];
        let targets = decision_branch_targets(&steps);
        assert!(targets.contains("b"));
        assert!(!targets.contains("c"));
        assert!(!targets.contains("d"));
    }
}
