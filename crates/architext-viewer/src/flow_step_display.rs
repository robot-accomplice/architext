//! Display model for a flow's ordered steps (the footer steps panel).
//!
//! Faithful port of `viewer/src/presentation/flowStepDisplayModel.js`:
//!  - [`flow_step_display_indexes`] — the per-step DISPLAY number, which folds a
//!    decision's branch outcomes back onto the decision step's own number so the
//!    `1, 2, 3a, 3b` reading the diagram shows is what the steps list shows.
//!  - [`is_decision_branch_support_step`] — whether a step is a decision-branch
//!    *support* step (an outcome edge sharing its decision's number) that the
//!    steps list hides (the JS list filters these out).
//!
//! Also a small [`glyph_for_step`] mapping the step kind to a single on-language
//! glyph for the steps-list kind icon (the Rust/WASM viewer renders a glyph, not
//! the full JS SVG icon set; the kind→semantic mapping is the same one
//! `diagramIconModel.js` `iconForStep` uses).

use std::collections::HashMap;

use crate::data::models::FlowStep;

/// Per-step display index, keyed by step id.
///
/// Port of `flowStepDisplayIndexes`. A step with an `outcome` (a decision
/// branch) takes the display number of the most recent decision step that
/// *targeted* its `from` node; every other step takes its 1-based position.
/// Decision steps record their number against the node they point at so the
/// following outcome edges fold back onto it.
pub fn flow_step_display_indexes(steps: &[FlowStep]) -> HashMap<String, usize> {
    let mut display_indexes = HashMap::with_capacity(steps.len());
    let mut latest_decision_index_by_node: HashMap<&str, usize> = HashMap::new();

    for (index, step) in steps.iter().enumerate() {
        let position = index + 1;
        let resolved = if step.outcome.is_some() {
            latest_decision_index_by_node
                .get(step.from.as_str())
                .copied()
                .unwrap_or(position)
        } else {
            position
        };
        display_indexes.insert(step.id.clone(), resolved);

        if step.kind.as_deref() == Some("decision") {
            latest_decision_index_by_node.insert(step.to.as_str(), position);
        }
    }

    display_indexes
}

/// Whether `step` (at `index`) is a decision-branch SUPPORT step the steps list
/// hides. Port of `isDecisionBranchSupportStep`: it is a support step iff it
/// carries an `outcome` AND its display index was folded back onto an earlier
/// decision (i.e. differs from its own 1-based position).
pub fn is_decision_branch_support_step(steps: &[FlowStep], step: &FlowStep, index: usize) -> bool {
    let display_index = flow_step_display_indexes(steps)
        .get(&step.id)
        .copied()
        .unwrap_or(index + 1);
    step.outcome.is_some() && display_index != index + 1
}

/// One row in the footer steps panel: the step's original index (for glyph
/// start/stop inference) and the number to render in the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepCardRow {
    /// 0-based index of the step in `flow.steps` (drives glyph + click target).
    pub index: usize,
    /// The number shown on the card.
    pub display_number: usize,
}

/// The ordered step rows the panel renders, mode-aware.
///
/// FLOWS / Data-Risks render a *routed* plan that folds a decision's branch
/// outcomes back onto the decision number (`3a/3b`), so the list hides those
/// support steps and uses the folded display index — see
/// [`flow_step_display_indexes`] / [`is_decision_branch_support_step`].
///
/// SEQUENCE renders EVERY step as its own time-ordered message row (no folding),
/// numbered 1-based in document order — exactly the
/// `sequence::MessageRow.number`. The panel must mirror that, so in sequence
/// mode it lists all steps with positional numbers. (Fixes the audit F5 where
/// the panel showed the Flows-filtered subset while the sequence diagram showed
/// all messages, so the two disagreed.)
pub fn step_card_rows(steps: &[FlowStep], is_sequence: bool) -> Vec<StepCardRow> {
    if is_sequence {
        return steps
            .iter()
            .enumerate()
            .map(|(index, _)| StepCardRow { index, display_number: index + 1 })
            .collect();
    }
    let display_indexes = flow_step_display_indexes(steps);
    steps
        .iter()
        .enumerate()
        .filter(|(i, s)| !is_decision_branch_support_step(steps, s, *i))
        .map(|(index, step)| StepCardRow {
            index,
            display_number: display_indexes.get(&step.id).copied().unwrap_or(index + 1),
        })
        .collect()
}

/// A single on-language glyph for a step's kind icon in the steps list.
///
/// Mirrors `iconForStep`: an explicit `kind` maps to its semantic glyph; with no
/// kind, the first step is a start and the last a stop, everything else a plain
/// process step.
pub fn glyph_for_step(step: &FlowStep, index: usize, total_steps: usize) -> &'static str {
    match step.kind.as_deref() {
        Some("start") => "▶",
        Some("stop") => "■",
        Some("decision") => "◆",
        Some("async") => "⇶",
        Some("persistence") => "▤",
        Some("artifact") => "◳",
        Some("return") => "↩",
        Some("process") => "▭",
        _ if index == 0 => "▶",
        _ if total_steps > 0 && index == total_steps - 1 => "■",
        _ => "▭",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, from: &str, to: &str, kind: Option<&str>, outcome: Option<&str>) -> FlowStep {
        FlowStep {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            action: format!("act-{id}"),
            summary: None,
            kind: kind.map(str::to_string),
            outcome: outcome.map(str::to_string),
            return_of: None,
        }
    }

    /// A linear flow: every step's display number is its 1-based position and
    /// none are support steps.
    #[test]
    fn linear_flow_has_positional_indexes_and_no_support_steps() {
        let steps = vec![
            step("s1", "a", "b", None, None),
            step("s2", "b", "c", None, None),
            step("s3", "c", "d", None, None),
        ];
        let idx = flow_step_display_indexes(&steps);
        assert_eq!(idx["s1"], 1);
        assert_eq!(idx["s2"], 2);
        assert_eq!(idx["s3"], 3);
        for (i, s) in steps.iter().enumerate() {
            assert!(!is_decision_branch_support_step(&steps, s, i));
        }
    }

    /// A decision with two outcome branches: both branch steps fold onto the
    /// decision's number (they are support steps the list hides), while the
    /// decision and the steps around it keep their positional numbers. This is
    /// WHY the filter exists — the diagram shows `3a/3b` on the decision's
    /// number, so the list must not re-list them as separate `4`/`5` cards.
    #[test]
    fn decision_branch_outcomes_fold_and_are_support_steps() {
        let steps = vec![
            step("s1", "a", "router", None, None),                 // 1
            step("s2", "router", "gate", Some("decision"), None),  // 2 (decision → gate)
            step("s3", "gate", "yes", None, Some("approved")),     // outcome → folds to 2
            step("s4", "gate", "no", None, Some("rejected")),      // outcome → folds to 2
            step("s5", "yes", "done", None, None),                 // 5
        ];
        let idx = flow_step_display_indexes(&steps);
        assert_eq!(idx["s1"], 1);
        assert_eq!(idx["s2"], 2);
        assert_eq!(idx["s3"], 2, "branch outcome folds onto the decision number");
        assert_eq!(idx["s4"], 2, "branch outcome folds onto the decision number");
        assert_eq!(idx["s5"], 5);

        // The two folded outcome edges are support steps (hidden in the list);
        // nothing else is.
        assert!(!is_decision_branch_support_step(&steps, &steps[0], 0));
        assert!(!is_decision_branch_support_step(&steps, &steps[1], 1));
        assert!(is_decision_branch_support_step(&steps, &steps[2], 2));
        assert!(is_decision_branch_support_step(&steps, &steps[3], 3));
        assert!(!is_decision_branch_support_step(&steps, &steps[4], 4));
    }

    /// An outcome step whose folded number equals its own position is NOT a
    /// support step — the filter keys on the number differing, not on `outcome`
    /// alone.
    #[test]
    fn outcome_step_keeping_its_position_is_not_a_support_step() {
        // No preceding decision targeted "a", so the outcome step keeps its own
        // position (1) and is not hidden.
        let steps = vec![step("s1", "a", "b", None, Some("only"))];
        assert!(!is_decision_branch_support_step(&steps, &steps[0], 0));
    }

    /// FLOWS mode: the panel hides decision-branch support steps and folds their
    /// numbers — the routed-plan reading. Same data as
    /// `decision_branch_outcomes_fold_and_are_support_steps`.
    #[test]
    fn step_card_rows_flows_mode_hides_support_steps_and_folds_numbers() {
        let steps = vec![
            step("s1", "a", "router", None, None),                 // 1
            step("s2", "router", "gate", Some("decision"), None),  // 2
            step("s3", "gate", "yes", None, Some("approved")),     // support → folds to 2
            step("s4", "gate", "no", None, Some("rejected")),      // support → folds to 2
            step("s5", "yes", "done", None, None),                 // 5
        ];
        let rows = step_card_rows(&steps, false);
        // s3 + s4 hidden → 3 cards, original indexes 0,1,4.
        assert_eq!(
            rows.iter().map(|r| (r.index, r.display_number)).collect::<Vec<_>>(),
            vec![(0, 1), (1, 2), (4, 5)]
        );
    }

    /// SEQUENCE mode: the panel mirrors the diagram's message rows — EVERY step,
    /// 1-based in document order, no folding/hiding. This is the F5 fix: with the
    /// same decision-branch data, sequence shows all 5 rows numbered 1..=5
    /// (matching `MessageRow.number`), not the Flows-filtered 3.
    #[test]
    fn step_card_rows_sequence_mode_lists_every_step_positionally() {
        let steps = vec![
            step("s1", "a", "router", None, None),
            step("s2", "router", "gate", Some("decision"), None),
            step("s3", "gate", "yes", None, Some("approved")),
            step("s4", "gate", "no", None, Some("rejected")),
            step("s5", "yes", "done", None, None),
        ];
        let rows = step_card_rows(&steps, true);
        assert_eq!(
            rows.iter().map(|r| (r.index, r.display_number)).collect::<Vec<_>>(),
            vec![(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]
        );
    }

    #[test]
    fn glyph_falls_back_to_start_stop_process_without_kind() {
        let steps = vec![
            step("s1", "a", "b", None, None),
            step("s2", "b", "c", None, None),
            step("s3", "c", "d", None, None),
        ];
        assert_eq!(glyph_for_step(&steps[0], 0, 3), "▶");
        assert_eq!(glyph_for_step(&steps[1], 1, 3), "▭");
        assert_eq!(glyph_for_step(&steps[2], 2, 3), "■");
    }

    #[test]
    fn glyph_uses_explicit_kind_over_position() {
        let s = step("s1", "a", "b", Some("decision"), None);
        assert_eq!(glyph_for_step(&s, 0, 3), "◆");
    }
}
