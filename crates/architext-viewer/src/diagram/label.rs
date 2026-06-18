//! Step-label renderer (one label per call).
//!
//! Each routed edge carries a label anchor (`labelX`, `labelY`) and a
//! `label_box` background rect from the plan. Flow-step labels collapse to the
//! step number and render as a compact mono badge (the full action text lives
//! in the steps-panel footer); structural labels (`"uses"`, `"depends on"`)
//! render as their text over the chrome-radius box so they stay legible across
//! edge crossings.

use architext_routing::model::Rect;
use leptos::*;

/// Radius of the flow number-pill badge (px in canvas space) — sized to fit two
/// digits, mirroring the steps-panel step-number idiom.
const BADGE_RADIUS: f64 = 11.0;

/// A label ready to render: its text, anchor, and background box. `is_number`
/// marks a collapsed flow-step pill (a digit-only badge) vs a structural text
/// label (box + text).
#[derive(Clone)]
pub struct LabelView {
    pub text: String,
    pub is_number: bool,
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub box_rect: Rect,
}

/// Render one edge label. A flow number-pill renders as a compact circular
/// badge centered on the anchor; a structural text label renders as text over a
/// rounded background box (4px chrome radius).
#[component]
pub fn DiagramLabel(label: LabelView) -> impl IntoView {
    if label.is_number {
        return view! {
            <g class="flow-label">
                <circle
                    class="flow-label__badge"
                    cx=label.anchor_x
                    cy=label.anchor_y
                    r=BADGE_RADIUS
                ></circle>
                <text class="flow-label__badge-num" x=label.anchor_x y=label.anchor_y>
                    {label.text.clone()}
                </text>
            </g>
        };
    }
    let Rect { x, y, width, height } = label.box_rect;
    view! {
        <g class="flow-label">
            <rect class="flow-label__box" x=x y=y width=width height=height rx="4" ry="4"></rect>
            <text class="flow-label__text" x=label.anchor_x y=label.anchor_y>
                {label.text.clone()}
            </text>
        </g>
    }
}
