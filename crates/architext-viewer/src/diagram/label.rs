//! Step-label renderer (one label per call).
//!
//! Each routed edge carries a label anchor (`labelX`, `labelY`) and a
//! `label_box` background rect from the plan. The label text is the engine's
//! step label (e.g. `"3. Persist record"`), rendered in JetBrains Mono over the
//! box so it stays legible across edge crossings.

use architext_routing::model::Rect;
use leptos::*;

/// A label ready to render: its text, anchor, and background box.
#[derive(Clone)]
pub struct LabelView {
    pub text: String,
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub box_rect: Rect,
}

/// Render one step label: a rounded background box (4px chrome radius) plus the
/// label text centered on the anchor.
#[component]
pub fn DiagramLabel(label: LabelView) -> impl IntoView {
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
