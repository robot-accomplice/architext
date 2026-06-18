//! Pure SEQUENCE-diagram layout (Leptos-free, native-testable).
//!
//! The sequence projection is NOT a `plan()` diagram — it is a custom
//! lifelines + time-ordered messages + frames layout. This module is the
//! faithful Rust port of the JS `SequenceDiagram()` layout math in
//! `viewer/src/main.tsx` plus the `sequenceActivationSpans` /
//! `sequenceStepMessageKind` / `sequenceReturnSourceStep` helpers in
//! `viewer/src/presentation/stepRouteModel.js`. It produces a [`SequenceLayout`]
//! render model (columns, lifelines, message rows, activation bars, frames, and
//! the content box) that the `SequenceSvg` component renders verbatim.
//!
//! The math is entirely linear (multiply/add), so — unlike the routing engine —
//! it needs no `js_round`/`ryu` byte-for-byte float bridging: replicating the
//! arithmetic exactly is sufficient.

use crate::data::models::{Flow, FlowStep, Node, SequenceFrame};
use std::collections::HashMap;

// ── Fixed layout constants (verbatim from the JS SequenceDiagram) ───────────
//
// The configurable dims (participant_width / row_height / margin_x) come from
// `/api/config`'s `diagram.sequence` block; these are the inline literals the
// JS hardcodes and are not configurable there either.

/// `headerY` — top of the participant header card band.
const HEADER_Y: f64 = 18.0;
/// `messageStartY` — y of the first message row (also the activation/​frame origin).
const MESSAGE_START_Y: f64 = 68.0;
/// Bottom padding added below the last message row (`+ 38` in the height calc).
const HEIGHT_TAIL: f64 = 38.0;
/// Lifeline top offset below the header (`headerY + 48`).
const LIFELINE_TOP_OFFSET: f64 = 48.0;
/// Lifeline bottom inset from the canvas bottom (`height - 22`).
const LIFELINE_BOTTOM_INSET: f64 = 22.0;
/// Participant header card half-width (the JS card is `width:116`, `left:x-58`).
const PARTICIPANT_CARD_HALF_WIDTH: f64 = 58.0;
/// Frame inset from the leftmost participant column (`+ 8`).
const FRAME_X_INSET: f64 = 8.0;
/// Frame width shrink (`- 16`, i.e. `8` per side).
const FRAME_WIDTH_INSET: f64 = 16.0;
/// Frame top lift above the first bracketed row (`- 30`).
const FRAME_Y_LIFT: f64 = 30.0;
/// Frame bottom extension below the last bracketed row (`+ 34`).
const FRAME_HEIGHT_TAIL: f64 = 34.0;
/// Activation-bar fixed width (`width="10"`).
const ACTIVATION_BAR_WIDTH: f64 = 10.0;
/// Activation-bar x nudge per depth level (`depth * 8`).
const ACTIVATION_DEPTH_STEP: f64 = 8.0;
/// Activation-bar x left-shift to straddle the lifeline (`- 5`).
const ACTIVATION_X_SHIFT: f64 = 5.0;
/// Activation span top lift (`index * rowHeight - 10`).
const ACTIVATION_Y_LIFT: f64 = 10.0;
/// Minimum activation-bar height (`Math.max(18, …)`).
const ACTIVATION_MIN_HEIGHT: f64 = 18.0;
/// Span end tail when there is no matching return (`rowHeight * 0.65`).
const ACTIVATION_OPEN_TAIL_FACTOR: f64 = 0.65;
/// Span end tail when a return closes the activation (`+ 10`).
const ACTIVATION_CLOSE_TAIL: f64 = 10.0;

/// The set of step `kind`s that map to a distinct sequence message kind. Any
/// other kind (or absent) collapses to `request`. Port of the JS
/// `sequenceMessageKinds` set + `sequenceStepMessageKind`.
const SEQUENCE_MESSAGE_KINDS: &[&str] = &["request", "return", "async", "persistence", "self"];

/// The resolved kind of a sequence message — drives the edge treatment
/// (solid request, dashed return/async, dotted persistence, self-loop).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageKind {
    Request,
    Return,
    Async,
    Persistence,
    /// `from == to` OR an explicit `kind: "self"` — rendered as a loopback.
    Selfloop,
}

impl MessageKind {
    /// Port of `sequenceStepMessageKind(step)` — purely `step.kind`-driven
    /// (the JS `fromX`/`toX` params are unused there). `self` and a from==to
    /// step both render as the self-loop treatment.
    fn from_step(step: &FlowStep) -> Self {
        match step.kind.as_deref() {
            Some(k) if SEQUENCE_MESSAGE_KINDS.contains(&k) => match k {
                "return" => MessageKind::Return,
                "async" => MessageKind::Async,
                "persistence" => MessageKind::Persistence,
                "self" => MessageKind::Selfloop,
                _ => MessageKind::Request,
            },
            _ => MessageKind::Request,
        }
    }

    /// CSS modifier suffix for the message group / line (`sequence-message--…`).
    pub fn css_suffix(self) -> &'static str {
        match self {
            MessageKind::Request => "request",
            MessageKind::Return => "return",
            MessageKind::Async => "async",
            MessageKind::Persistence => "persistence",
            MessageKind::Selfloop => "self",
        }
    }
}

/// A participant column: its x-center, the resolved display name + role type
/// (for the `--c4-*` header tint), and the underlying node id (for click →
/// inspector).
#[derive(Clone, Debug, PartialEq)]
pub struct ParticipantColumn {
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub x: f64,
}

/// A lifeline (the vertical line dropped from a participant header to the footer).
#[derive(Clone, Debug, PartialEq)]
pub struct Lifeline {
    pub x: f64,
    pub y1: f64,
    pub y2: f64,
}

/// A laid-out message row (one flow step). `index` is the 1-based step number
/// rendered in the mid-line chip; geometry keys off the 0-based row.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageRow {
    pub step_id: String,
    pub from: String,
    pub to: String,
    pub action: String,
    pub number: usize,
    pub from_x: f64,
    pub to_x: f64,
    pub y: f64,
    pub mid_x: f64,
    pub kind: MessageKind,
}

/// An activation bar straddling a lifeline for the duration a participant is
/// "active" (between a request and its matching return).
#[derive(Clone, Debug, PartialEq)]
pub struct ActivationBar {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A bordered frame box (alt/loop/par/opt/transaction) spanning a step range.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameBox {
    pub id: String,
    pub frame_type: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The complete render model for one sequence diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct SequenceLayout {
    pub content_width: f64,
    pub content_height: f64,
    pub participants: Vec<ParticipantColumn>,
    pub lifelines: Vec<Lifeline>,
    pub messages: Vec<MessageRow>,
    pub activation_bars: Vec<ActivationBar>,
    pub frames: Vec<FrameBox>,
}

/// The configurable sequence dimensions (from `/api/config`'s
/// `diagram.sequence`, defaults applied). Mirrors the JS `useDiagramConfig().sequence`.
#[derive(Clone, Copy, Debug)]
pub struct SequenceConfig {
    pub participant_width: f64,
    pub row_height: f64,
    pub margin_x: f64,
}

impl Default for SequenceConfig {
    /// The JS inline defaults (`?? 146`, `?? 56`, `?? 28`).
    fn default() -> Self {
        Self { participant_width: 146.0, row_height: 56.0, margin_x: 28.0 }
    }
}

impl SequenceConfig {
    /// Resolve from the `/api/config` `diagram` value's `sequence` block,
    /// falling through to the JS defaults for any absent/non-numeric field —
    /// the same pattern `plan::layout_config_from_diagram` uses for layout.
    pub fn from_diagram(diagram: &serde_json::Value) -> Self {
        let seq = diagram.get("sequence");
        let d = Self::default();
        let num = |key: &str, fallback: f64| -> f64 {
            seq.and_then(|s| s.get(key)).and_then(serde_json::Value::as_f64).unwrap_or(fallback)
        };
        Self {
            participant_width: num("participantWidth", d.participant_width),
            row_height: num("rowHeight", d.row_height),
            margin_x: num("marginX", d.margin_x),
        }
    }
}

/// An intermediate activation span before pixel placement — the direct port of
/// the `sequenceActivationSpans` data shape.
struct ActivationSpan<'a> {
    step_id: &'a str,
    participant_id: &'a str,
    start_index: usize,
    end_index: usize,
    y1: f64,
    y2: f64,
}

/// Port of `sequenceReturnSourceStep(step, priorSteps)` — find the step a
/// `return` answers: by explicit `returnOf`, else the most-recent prior step
/// with the mirrored from/to.
fn return_source_index(step: &FlowStep, prior: &[FlowStep]) -> Option<usize> {
    if let Some(ref ret_of) = step.return_of {
        return prior.iter().position(|c| &c.id == ret_of);
    }
    // Most recent prior step with mirrored endpoints (reverse scan).
    prior
        .iter()
        .enumerate()
        .rev()
        .find(|(_, c)| c.from == step.to && c.to == step.from)
        .map(|(i, _)| i)
}

/// Port of `sequenceActivationSpans(steps, rowHeight)`. Returns spans paired
/// with their nesting `depth` (count of enclosing same-participant spans).
fn activation_spans(steps: &[FlowStep], row_height: f64) -> Vec<(ActivationSpan<'_>, usize)> {
    // Map source step id → the index of the return that closes it (first match).
    let mut return_end_by_source: HashMap<&str, usize> = HashMap::new();
    for (index, step) in steps.iter().enumerate() {
        if MessageKind::from_step(step) != MessageKind::Return {
            continue;
        }
        if let Some(src_i) = return_source_index(step, &steps[..index]) {
            let src_id = steps[src_i].id.as_str();
            return_end_by_source.entry(src_id).or_insert(index);
        }
    }

    // Base spans: every non-return step opens an activation on its target
    // (or, for a self message, its source).
    let base: Vec<ActivationSpan<'_>> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let kind = MessageKind::from_step(step);
            if kind == MessageKind::Return {
                return None;
            }
            let participant_id = if kind == MessageKind::Selfloop {
                step.from.as_str()
            } else {
                step.to.as_str()
            };
            let end_index = return_end_by_source.get(step.id.as_str()).copied();
            let has_return = end_index.is_some();
            let end = end_index.unwrap_or(index);
            let y2 = (end as f64) * row_height
                + if has_return {
                    ACTIVATION_CLOSE_TAIL
                } else {
                    row_height * ACTIVATION_OPEN_TAIL_FACTOR
                };
            Some(ActivationSpan {
                step_id: step.id.as_str(),
                participant_id,
                start_index: index,
                end_index: end,
                y1: (index as f64) * row_height - ACTIVATION_Y_LIFT,
                y2,
            })
        })
        .collect();

    // Depth = number of earlier spans on the same participant that enclose this
    // span's start row.
    base.iter()
        .enumerate()
        .map(|(i, span)| {
            let depth = base[..i]
                .iter()
                .filter(|c| {
                    c.participant_id == span.participant_id
                        && c.start_index <= span.start_index
                        && c.end_index >= span.start_index
                })
                .count();
            // SAFETY of the borrow: we rebuild a fresh struct referencing the
            // same string slices (all borrow from `steps`).
            (
                ActivationSpan {
                    step_id: span.step_id,
                    participant_id: span.participant_id,
                    start_index: span.start_index,
                    end_index: span.end_index,
                    y1: span.y1,
                    y2: span.y2,
                },
                depth,
            )
        })
        .collect()
}

/// Build the full [`SequenceLayout`] for a flow. `nodes_by_id` resolves
/// participant display names + role types; unresolved ids fall back to the id
/// itself with a `node` type (matching the JS `node?.name ?? id` /
/// `node?.type ?? "node"`).
pub fn build_sequence_layout(
    flow: &Flow,
    nodes_by_id: &HashMap<&str, &Node>,
    config: &SequenceConfig,
) -> SequenceLayout {
    let participant_width = config.participant_width;
    let row_height = config.row_height;
    let margin_x = config.margin_x;

    // participantIds = distinct from/to in step order (insertion-ordered set).
    let mut participant_ids: Vec<&str> = Vec::new();
    for step in &flow.steps {
        for id in [step.from.as_str(), step.to.as_str()] {
            if !participant_ids.contains(&id) {
                participant_ids.push(id);
            }
        }
    }

    let index_of = |id: &str| -> Option<usize> { participant_ids.iter().position(|p| *p == id) };
    let x_for = |id: &str| -> f64 {
        let i = index_of(id).unwrap_or(0) as f64;
        margin_x + i * participant_width + participant_width / 2.0
    };
    let y_for_step = |index: usize| -> f64 { MESSAGE_START_Y + (index as f64) * row_height };

    let content_width = margin_x * 2.0 + (participant_ids.len() as f64) * participant_width;
    let content_height = MESSAGE_START_Y + (flow.steps.len() as f64) * row_height + HEIGHT_TAIL;

    // Participant columns + lifelines.
    let lifeline_top = HEADER_Y + LIFELINE_TOP_OFFSET;
    let lifeline_bottom = content_height - LIFELINE_BOTTOM_INSET;
    let mut participants = Vec::with_capacity(participant_ids.len());
    let mut lifelines = Vec::with_capacity(participant_ids.len());
    for id in &participant_ids {
        let node = nodes_by_id.get(id);
        let x = x_for(id);
        participants.push(ParticipantColumn {
            id: id.to_string(),
            name: node.map(|n| n.name.clone()).unwrap_or_else(|| id.to_string()),
            node_type: node.map(|n| n.node_type.clone()).unwrap_or_else(|| "node".to_string()),
            x,
        });
        lifelines.push(Lifeline { x, y1: lifeline_top, y2: lifeline_bottom });
    }

    // step id → index, for frame span resolution.
    let step_index: HashMap<&str, usize> =
        flow.steps.iter().enumerate().map(|(i, s)| (s.id.as_str(), i)).collect();

    // Frames (alt/loop/par/opt/transaction). Skipped if no listed step resolves.
    let frames = flow
        .sequence_frames
        .iter()
        .filter_map(|frame| build_frame(frame, flow, &step_index, &index_of, &y_for_step, margin_x, participant_width))
        .collect();

    // Activation bars: place each span into pixels.
    let activation_bars = activation_spans(&flow.steps, row_height)
        .into_iter()
        .map(|(span, depth)| ActivationBar {
            id: format!("activation-{}", span.step_id),
            x: x_for(span.participant_id) + (depth as f64) * ACTIVATION_DEPTH_STEP - ACTIVATION_X_SHIFT,
            y: MESSAGE_START_Y + span.y1,
            width: ACTIVATION_BAR_WIDTH,
            height: (span.y2 - span.y1).max(ACTIVATION_MIN_HEIGHT),
        })
        .collect();

    // Message rows (document order; geometry keyed off the original index).
    let messages = flow
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let from_x = x_for(&step.from);
            let to_x = x_for(&step.to);
            let y = MESSAGE_START_Y + (index as f64) * row_height;
            // from==to is always a self-loop, regardless of declared kind.
            let kind = if step.from == step.to {
                MessageKind::Selfloop
            } else {
                MessageKind::from_step(step)
            };
            MessageRow {
                step_id: step.id.clone(),
                from: step.from.clone(),
                to: step.to.clone(),
                action: step.action.clone(),
                number: index + 1,
                from_x,
                to_x,
                y,
                mid_x: (from_x + to_x) / 2.0,
                kind,
            }
        })
        .collect();

    SequenceLayout {
        content_width,
        content_height,
        participants,
        lifelines,
        messages,
        activation_bars,
        frames,
    }
}

/// Build one frame box, or `None` if no listed step resolves (JS `flatMap` +
/// early `[]`).
#[allow(clippy::too_many_arguments)]
fn build_frame(
    frame: &SequenceFrame,
    flow: &Flow,
    step_index: &HashMap<&str, usize>,
    index_of: &impl Fn(&str) -> Option<usize>,
    y_for_step: &impl Fn(usize) -> f64,
    margin_x: f64,
    participant_width: f64,
) -> Option<FrameBox> {
    let indexes: Vec<usize> = frame
        .step_ids
        .iter()
        .filter_map(|sid| step_index.get(sid.as_str()).copied())
        .collect();
    if indexes.is_empty() {
        return None;
    }
    // Participant column indexes touched by the bracketed steps (from + to).
    let participant_indexes: Vec<usize> = frame
        .step_ids
        .iter()
        .filter_map(|sid| step_index.get(sid.as_str()).copied())
        .flat_map(|i| {
            let step = &flow.steps[i];
            [index_of(&step.from), index_of(&step.to)]
        })
        .flatten()
        .collect();
    let min_participant = *participant_indexes.iter().min()?;
    let max_participant = *participant_indexes.iter().max()?;
    let min_index = *indexes.iter().min()?;
    let max_index = *indexes.iter().max()?;

    let x = margin_x + (min_participant as f64) * participant_width + FRAME_X_INSET;
    let width = ((max_participant - min_participant + 1) as f64) * participant_width - FRAME_WIDTH_INSET;
    let y = y_for_step(min_index) - FRAME_Y_LIFT;
    let height = y_for_step(max_index) - y + FRAME_HEIGHT_TAIL;

    Some(FrameBox {
        id: frame.id.clone(),
        frame_type: frame.frame_type.clone(),
        label: frame.label.clone().unwrap_or_default(),
        x,
        y,
        width,
        height,
    })
}

/// The participant header card half-width (the renderer offsets `x` by this to
/// place the left edge, matching the JS `left: x - 58`).
pub const CARD_HALF_WIDTH: f64 = PARTICIPANT_CARD_HALF_WIDTH;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{Flow, FlowStep, Node, SequenceFrame};
    use std::collections::HashMap;

    fn node(id: &str, name: &str, ty: &str) -> Node {
        Node {
            id: id.to_string(),
            node_type: ty.to_string(),
            name: name.to_string(),
            summary: None,
            owner: None,
            dependencies: vec![],
            source_paths: vec![],
            related_flows: vec![],
            related_decisions: vec![],
            known_risks: vec![],
            data_handled: vec![],
        }
    }

    fn step(id: &str, from: &str, to: &str, action: &str, kind: Option<&str>) -> FlowStep {
        FlowStep {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            action: action.to_string(),
            summary: None,
            kind: kind.map(str::to_string),
            outcome: None,
            return_of: None,
        }
    }

    /// The real corpus flow `fresh-install` (6 steps, includes a `persistence`
    /// step) projected against the `sequence` view's participants.
    fn fresh_install_like() -> (Flow, Vec<Node>) {
        let steps = vec![
            step("s1", "maintainer", "architext-cli", "run install", Some("start")),
            step("s2", "architext-cli", "target-data-files", "write data", Some("persistence")),
            step("s3", "architext-cli", "static-dist", "emit dist", Some("artifact")),
            step("s4", "architext-cli", "schema-validator", "validate?", Some("decision")),
            step("s5", "schema-validator", "architext-cli", "ok", Some("process")),
            step("s6", "architext-cli", "maintainer", "done", Some("stop")),
        ];
        let nodes = vec![
            node("maintainer", "Maintainer", "actor"),
            node("architext-cli", "Architext CLI", "service"),
            node("target-data-files", "Data Files", "data"),
            node("static-dist", "Static Dist", "data"),
            node("schema-validator", "Schema Validator", "service"),
        ];
        let flow = Flow {
            id: "fresh-install".to_string(),
            name: "Fresh Install".to_string(),
            status: None,
            summary: None,
            trigger: None,
            steps,
            sequence_frames: vec![],
        };
        (flow, nodes)
    }

    #[test]
    fn layout_for_corpus_like_flow_has_expected_shape() {
        let (flow, nodes) = fresh_install_like();
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let cfg = SequenceConfig::default();
        let layout = build_sequence_layout(&flow, &by_id, &cfg);

        // 5 distinct participants in first-touch order.
        assert_eq!(
            layout.participants.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec![
                "maintainer",
                "architext-cli",
                "target-data-files",
                "static-dist",
                "schema-validator"
            ]
        );
        // One message row per step, numbered 1..=6, geometry monotonic in y.
        assert_eq!(layout.messages.len(), flow.steps.len());
        assert_eq!(layout.messages[0].number, 1);
        assert_eq!(layout.messages[5].number, 6);
        // Content box is positive and matches the JS formula.
        assert!(layout.content_width > 0.0 && layout.content_height > 0.0);
        assert_eq!(layout.content_width, 28.0 * 2.0 + 5.0 * 146.0);
        assert_eq!(layout.content_height, 68.0 + 6.0 * 56.0 + 38.0);
        // One lifeline per participant.
        assert_eq!(layout.lifelines.len(), 5);
        // Role types are single-sourced from the node.type.
        assert_eq!(layout.participants[0].node_type, "actor");
    }

    #[test]
    fn x_for_matches_js_formula() {
        let (flow, nodes) = fresh_install_like();
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let cfg = SequenceConfig::default();
        let layout = build_sequence_layout(&flow, &by_id, &cfg);
        // xFor(index 0) = 28 + 0*146 + 73 = 101
        assert_eq!(layout.participants[0].x, 28.0 + 0.0 * 146.0 + 146.0 / 2.0);
        // xFor(index 2) = 28 + 2*146 + 73 = 393
        assert_eq!(layout.participants[2].x, 28.0 + 2.0 * 146.0 + 146.0 / 2.0);
        // yForStepIndex(0) = 68; row 3 = 68 + 3*56 = 236
        assert_eq!(layout.messages[0].y, 68.0);
        assert_eq!(layout.messages[3].y, 68.0 + 3.0 * 56.0);
    }

    #[test]
    fn persistence_step_keeps_its_kind_others_collapse_to_request() {
        let (flow, nodes) = fresh_install_like();
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let layout = build_sequence_layout(&flow, &by_id, &SequenceConfig::default());
        // s2 is persistence (in the set) → Persistence; s1 start (not in set) → Request.
        assert_eq!(layout.messages[1].kind, MessageKind::Persistence);
        assert_eq!(layout.messages[0].kind, MessageKind::Request);
        assert_eq!(layout.messages[3].kind, MessageKind::Request); // decision → request
    }

    #[test]
    fn return_step_closes_activation_and_renders_dashed() {
        // request a→b, then return b→a: the activation on `b` closes at the return.
        let flow = Flow {
            id: "f".into(),
            name: "f".into(),
            status: None,
            summary: None,
            trigger: None,
            steps: vec![
                step("q", "a", "b", "ask", Some("request")),
                step("r", "b", "a", "answer", Some("return")),
            ],
            sequence_frames: vec![],
        };
        let nodes = vec![node("a", "A", "actor"), node("b", "B", "service")];
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let layout = build_sequence_layout(&flow, &by_id, &SequenceConfig::default());
        assert_eq!(layout.messages[1].kind, MessageKind::Return);
        // One activation bar (the return itself opens none); it closes via the +10 tail.
        assert_eq!(layout.activation_bars.len(), 1);
        let bar = &layout.activation_bars[0];
        // span: y1 = 0*56 - 10 = -10 ; y2 = 1*56 + 10 = 66 ; height = 76.
        assert_eq!(bar.height, 66.0 - (-10.0));
    }

    #[test]
    fn self_message_detected_from_equal_endpoints() {
        let flow = Flow {
            id: "f".into(),
            name: "f".into(),
            status: None,
            summary: None,
            trigger: None,
            steps: vec![step("s", "a", "a", "tick", None)],
            sequence_frames: vec![],
        };
        let nodes = vec![node("a", "A", "service")];
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let layout = build_sequence_layout(&flow, &by_id, &SequenceConfig::default());
        assert_eq!(layout.messages[0].kind, MessageKind::Selfloop);
        assert_eq!(layout.messages[0].from_x, layout.messages[0].to_x);
    }

    #[test]
    fn frame_spans_step_range_and_participant_range() {
        let flow = Flow {
            id: "f".into(),
            name: "f".into(),
            status: None,
            summary: None,
            trigger: None,
            steps: vec![
                step("s1", "a", "b", "one", None),
                step("s2", "b", "c", "two", None),
                step("s3", "c", "a", "three", None),
            ],
            sequence_frames: vec![SequenceFrame {
                id: "fr1".into(),
                frame_type: "loop".into(),
                label: Some("retry".into()),
                step_ids: vec!["s1".into(), "s2".into()],
            }],
        };
        let nodes = vec![node("a", "A", "actor"), node("b", "B", "service"), node("c", "C", "data")];
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let layout = build_sequence_layout(&flow, &by_id, &SequenceConfig::default());
        assert_eq!(layout.frames.len(), 1);
        let f = &layout.frames[0];
        assert_eq!(f.frame_type, "loop");
        assert_eq!(f.label, "retry");
        // participants a(0),b(1),c(2) touched by s1,s2 → min 0, max 2.
        // x = 28 + 0*146 + 8 = 36 ; width = (2-0+1)*146 - 16 = 422.
        assert_eq!(f.x, 28.0 + 8.0);
        assert_eq!(f.width, 3.0 * 146.0 - 16.0);
        // y = yForStep(0) - 30 = 38 ; height = yForStep(1) - 38 + 34 = 124 - 38 + 34 = 120.
        assert_eq!(f.y, 68.0 - 30.0);
        assert_eq!(f.height, (68.0 + 56.0) - 38.0 + 34.0);
    }

    #[test]
    fn frame_dropped_when_no_step_resolves() {
        let flow = Flow {
            id: "f".into(),
            name: "f".into(),
            status: None,
            summary: None,
            trigger: None,
            steps: vec![step("s1", "a", "b", "one", None)],
            sequence_frames: vec![SequenceFrame {
                id: "fr1".into(),
                frame_type: "alt".into(),
                label: None,
                step_ids: vec!["missing".into()],
            }],
        };
        let nodes = vec![node("a", "A", "actor"), node("b", "B", "service")];
        let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let layout = build_sequence_layout(&flow, &by_id, &SequenceConfig::default());
        assert!(layout.frames.is_empty());
    }

    #[test]
    fn config_from_diagram_reads_sequence_block() {
        let diagram = serde_json::json!({
            "sequence": { "participantWidth": 200, "rowHeight": 40, "marginX": 10 }
        });
        let cfg = SequenceConfig::from_diagram(&diagram);
        assert_eq!(cfg.participant_width, 200.0);
        assert_eq!(cfg.row_height, 40.0);
        assert_eq!(cfg.margin_x, 10.0);
        // Missing block → defaults.
        let empty = SequenceConfig::from_diagram(&serde_json::Value::Null);
        assert_eq!(empty.participant_width, 146.0);
    }
}
