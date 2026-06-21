//! The SEQUENCE diagram `<svg>`: fluid viewBox + pan/zoom transform group,
//! lifelines, frames, activation bars, and message rows.
//!
//! Same fluid-canvas contract as [`super::svg::DiagramSvg`] (DESIGN.md rule 3):
//! `width=100%`/`height=100%` with a `viewBox` sized to the layout's content
//! box; pan/zoom is a single `transform` on the inner `<g>` driven by the
//! canvas-panel signals.
//!
//! Message lines use `vector-effect: non-scaling-stroke` + `--edge` so they
//! stay visible at any zoom (the same fix the flows edges got). Participant
//! role color is single-sourced through [`super::role_color_var`]; selection is
//! a STATE treatment, never a role hue.

use leptos::*;

use super::role_color_var;
use super::sequence::{
    ActivationBar, FrameBox, Lifeline, MessageKind, MessageRow, ParticipantColumn, SequenceLayout,
    CARD_HALF_WIDTH,
};

/// Participant header card height (visual band below `HEADER_Y`).
const CARD_HEIGHT: f64 = 40.0;
/// Card top y (just below the canvas top).
const CARD_TOP: f64 = 14.0;
/// Bottom of the participant header band (cards span `CARD_TOP..CARD_TOP +
/// CARD_HEIGHT`). The message/activation/frame layers are clipped to start here
/// so a first-row action label (drawn 17px above its line at `MESSAGE_START_Y`,
/// i.e. y≈51) can never bleed up into the header band and peek out behind the
/// participant cards / top activation bar (the `eTarg…` clipping artifact).
const HEADER_BAND_BOTTOM: f64 = CARD_TOP + CARD_HEIGHT;
/// Truncation cap for the action label (JS slices to 23 + "…" past 26).
const ACTION_MAX: usize = 26;
const ACTION_KEEP: usize = 23;

/// Truncate the message action like the JS (`> 26 → slice(0,23) + "..."`).
fn truncate_action(action: &str) -> String {
    let chars: Vec<char> = action.chars().collect();
    if chars.len() > ACTION_MAX {
        let kept: String = chars[..ACTION_KEEP].iter().collect();
        format!("{kept}...")
    } else {
        action.to_string()
    }
}

#[component]
pub fn SequenceSvg(
    layout: SequenceLayout,
    #[prop(into)] pan_x: Signal<f64>,
    #[prop(into)] pan_y: Signal<f64>,
    #[prop(into)] zoom: Signal<f64>,
    #[prop(into)] selected_node: Signal<Option<String>>,
    /// The steps-panel selection (step id). The matching message row gets the
    /// `sequence-message--active` STATE class — the sequence analogue of the
    /// flows `flow-edge--active` (rule 1: `--accent`, never a role hue).
    #[prop(into)] selected_step: Signal<Option<String>>,
    #[prop(into)] on_select: Callback<String>,
    /// Click a message → select its step (diagram → panel highlight), mirroring
    /// the steps-panel's click → highlight (panel → diagram).
    #[prop(into)] on_select_step: Callback<String>,
) -> impl IntoView {
    let view_box = format!("0 0 {} {}", layout.content_width, layout.content_height);
    let transform =
        move || format!("translate({} {}) scale({})", pan_x.get(), pan_y.get(), zoom.get());
    // Clip the body layers (frames / activations / messages) so nothing renders
    // above the header band. Height is generous (the full content box) — only the
    // top edge matters. Lifelines are NOT clipped: they intentionally start just
    // below the band and run the full height.
    let body_clip_height = (layout.content_height - HEADER_BAND_BOTTOM).max(0.0);
    let body_clip_width = layout.content_width;

    let participants = layout.participants.clone();
    let lifelines = layout.lifelines.clone();
    let frames = layout.frames.clone();
    let bars = layout.activation_bars.clone();
    let messages = layout.messages.clone();

    view! {
        <svg class="flow-svg sequence-svg" viewBox=view_box preserveAspectRatio="xMidYMid meet">
            <defs>
                <marker
                    id="sequence-arrowhead"
                    viewBox="0 0 10 10"
                    refX="9" refY="5"
                    markerWidth="7" markerHeight="7"
                    markerUnits="userSpaceOnUse"
                    orient="auto-start-reverse"
                >
                    <path d="M 0 0 L 10 5 L 0 10 z" class="flow-arrowhead"></path>
                </marker>
                // OPEN (stick) arrowhead for RETURN/async messages — the UML
                // convention (filled head = synchronous call, open head = reply/async).
                <marker
                    id="sequence-arrowhead-open"
                    viewBox="0 0 10 10"
                    refX="9" refY="5"
                    markerWidth="9" markerHeight="9"
                    markerUnits="userSpaceOnUse"
                    orient="auto-start-reverse"
                >
                    <path d="M 0 0 L 10 5 L 0 10" class="sequence-arrowhead-open"></path>
                </marker>
                // Clip the body layers to below the participant header band so a
                // first-row action label can't bleed up behind the header cards.
                <clipPath id="sequence-body-clip">
                    <rect x="0" y=HEADER_BAND_BOTTOM width=body_clip_width height=body_clip_height></rect>
                </clipPath>
            </defs>
            <g class="sequence-transform" transform=transform>
                // Z-order: lifelines, frames, activation bars, messages, then
                // participant header cards on top.
                <g class="sequence-lifelines">
                    {lifelines.into_iter().map(|l| view! { <SeqLifeline line=l/> }).collect_view()}
                </g>
                // Body layers (frames / activations / messages) clipped to below
                // the header band — keeps stray top-row labels out of the header.
                <g class="sequence-body" clip-path="url(#sequence-body-clip)">
                    <g class="sequence-frames">
                        {frames.into_iter().map(|f| view! { <SeqFrame frame=f/> }).collect_view()}
                    </g>
                    <g class="sequence-activations">
                        {bars.into_iter().map(|b| view! { <SeqActivationBar bar=b/> }).collect_view()}
                    </g>
                    <g class="sequence-messages">
                        {messages.into_iter().map(|m| {
                            let step_id = m.step_id.clone();
                            let is_selected = Signal::derive(move || {
                                selected_step.get().as_deref() == Some(step_id.as_str())
                            });
                            view! {
                                <SeqMessage message=m selected=is_selected on_select_step=on_select_step/>
                            }
                        }).collect_view()}
                    </g>
                </g>
                <g class="sequence-participants">
                    {participants.into_iter().map(|p| {
                        let id = p.id.clone();
                        let is_selected = Signal::derive(move || {
                            selected_node.get().as_deref() == Some(id.as_str())
                        });
                        view! { <SeqParticipant participant=p selected=is_selected on_select=on_select/> }
                    }).collect_view()}
                </g>
            </g>
        </svg>
    }
}

#[component]
fn SeqLifeline(line: Lifeline) -> impl IntoView {
    view! {
        <line class="sequence-lifeline" x1=line.x y1=line.y1 x2=line.x y2=line.y2></line>
    }
}

/// Operator-tab pentagon height + folded-corner cut (UML interaction-operator label).
const FRAME_TAB_HEIGHT: f64 = 18.0;
const FRAME_TAB_CUT: f64 = 6.0;
/// Approx mono glyph advance at 10px, for sizing the tab to the operator text.
const FRAME_TAB_CHAR_W: f64 = 6.2;
/// Horizontal padding inside the operator tab.
const FRAME_TAB_PAD: f64 = 14.0;

#[component]
fn SeqFrame(frame: FrameBox) -> impl IntoView {
    let op = frame.frame_type.clone();
    let label = frame.label.clone();
    let has_label = !label.is_empty();
    // UML operator tab: a pentagon with a folded bottom-right corner, sized to the
    // operator text, at the fragment's top-left. The operator sits inside it; the
    // fragment guard/title renders to its right.
    let tab_w = (op.chars().count() as f64) * FRAME_TAB_CHAR_W + FRAME_TAB_PAD;
    let (x, y) = (frame.x, frame.y);
    let tab_d = format!(
        "M {x} {y} H {right} V {fold_y} L {fold_x} {bottom} H {x} Z",
        right = x + tab_w,
        fold_y = y + FRAME_TAB_HEIGHT - FRAME_TAB_CUT,
        fold_x = x + tab_w - FRAME_TAB_CUT,
        bottom = y + FRAME_TAB_HEIGHT,
    );
    let op_cx = x + tab_w / 2.0;
    let text_y = y + 12.5;
    // Guard/title is RIGHT-aligned to the fragment's right edge: the first bracketed
    // message's action label sits centre-left in the top band, so anchoring the title
    // to the right keeps the two from overprinting.
    let label_x = x + frame.width - 8.0;
    view! {
        <g class=format!("sequence-frame sequence-frame--{}", frame.frame_type)>
            <rect
                class="sequence-frame__box"
                x=frame.x y=frame.y width=frame.width height=frame.height rx="3"
            ></rect>
            <path class="sequence-frame__tab" d=tab_d></path>
            <text class="sequence-frame__op" x=op_cx y=text_y>{op}</text>
            {has_label.then(|| view! {
                <text class="sequence-frame__label" x=label_x y=text_y>{label}</text>
            })}
        </g>
    }
}

#[component]
fn SeqActivationBar(bar: ActivationBar) -> impl IntoView {
    view! {
        <rect
            class="sequence-activation-bar"
            x=bar.x y=bar.y width=bar.width height=bar.height rx="1.5"
        ></rect>
    }
}

#[component]
fn SeqMessage(
    message: MessageRow,
    /// Whether this message's step is the steps-panel selection. Drives the
    /// `sequence-message--active` STATE class (mirrors flows `flow-edge--active`).
    #[prop(into)] selected: Signal<bool>,
    /// Click → select this step (diagram → panel highlight).
    #[prop(into)] on_select_step: Callback<String>,
) -> impl IntoView {
    let MessageRow { step_id, from_x, to_x, y, mid_x, number, action, kind, .. } = message;
    let base_class = format!("sequence-message sequence-message--{}", kind.css_suffix());
    let group_class = move || {
        if selected.get() {
            format!("{base_class} sequence-message--active")
        } else {
            base_class.clone()
        }
    };
    let line_class = format!("sequence-line sequence-line--{}", kind.css_suffix());
    // UML: filled head for a synchronous call, OPEN head for a return/async reply.
    let marker_end = match kind {
        MessageKind::Return | MessageKind::Async => "url(#sequence-arrowhead-open)",
        _ => "url(#sequence-arrowhead)",
    };
    let action_text = truncate_action(&action);
    let step_id_for_click = step_id.clone();

    // Self-loop: a small rectangular loopback on the participant's own lifeline.
    // Otherwise a straight horizontal arrow at the row y.
    let line_view = if kind == MessageKind::Selfloop {
        let loop_w = 26.0;
        let loop_h = 18.0;
        let d = format!(
            "M {x} {y} h {w} v {h} h -{w}",
            x = from_x,
            y = y - loop_h / 2.0,
            w = loop_w,
            h = loop_h,
        );
        view! {
            <path class=line_class.clone() d=d fill="none" marker-end=marker_end></path>
        }
        .into_view()
    } else {
        view! {
            <line
                class=line_class.clone()
                x1=from_x y1=y x2=to_x y2=y
                marker-end=marker_end
            ></line>
        }
        .into_view()
    };

    view! {
        <g class=group_class on:click=move |_| on_select_step.call(step_id_for_click.clone())>
            {line_view}
            // Action label above the line.
            <text class="sequence-action" x=mid_x y=y - 17.0>{action_text}</text>
            // Step-number chip on the line midpoint (JetBrains Mono via CSS).
            <g class="sequence-step-chip" transform=format!("translate({mid_x} {y})")>
                <circle class="sequence-step-chip__dot" r="9"></circle>
                <text class="sequence-step-chip__num" x="0" y="0">{number.to_string()}</text>
            </g>
        </g>
    }
}

#[component]
fn SeqParticipant(
    participant: ParticipantColumn,
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let ParticipantColumn { id, name, node_type, x } = participant;
    let role = role_color_var(&node_type);
    let left = x - CARD_HALF_WIDTH;
    let card_w = CARD_HALF_WIDTH * 2.0;
    let id_for_click = id.clone();
    let group_class = move || {
        if selected.get() {
            "sequence-participant sequence-participant--selected"
        } else {
            "sequence-participant"
        }
    };

    view! {
        <g
            class=group_class
            transform=format!("translate({left} {CARD_TOP})")
            on:click=move |_| on_select.call(id_for_click.clone())
        >
            <rect class="sequence-participant__card" width=card_w height=CARD_HEIGHT rx="6"></rect>
            // 2px role top-bar (single-sourced role color), same idiom as node cards.
            <rect class="sequence-participant__topbar" width=card_w height="2" fill=role.clone()></rect>
            <text class="sequence-participant__name" x=card_w / 2.0 y="17">{name}</text>
            <text class="sequence-participant__type" x=card_w / 2.0 y="31" fill=role>{node_type}</text>
        </g>
    }
}
