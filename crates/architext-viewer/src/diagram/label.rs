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

use crate::components::relationship_icon::RelationshipKind;

/// Radius of the flow number-pill badge (px in canvas space) — sized to fit two
/// digits, mirroring the steps-panel step-number idiom.
const BADGE_RADIUS: f64 = 11.0;

/// Side length of the square structural relationship-glyph pill (canvas px) and
/// the inset of the 24×24 glyph inside it.
const REL_PILL_SIZE: f64 = 18.0;
const REL_GLYPH_PAD: f64 = 3.0;

/// How a label renders. Flow-step labels collapse to a compact `Number` badge
/// (the full action text lives in the steps panel); structural (C4 /
/// deployment) relationship labels render as a `Relationship` GLYPH pill (the
/// word as a hover `title`), so dense diagrams stay legible.
#[derive(Clone)]
pub enum LabelKind {
    /// A collapsed flow-step number badge (the digit string).
    Number(String),
    /// A structural relationship: its semantic kind (→ glyph) plus the original
    /// word (→ hover title / legend).
    Relationship { kind: RelationshipKind, word: String },
}

/// A label ready to render: its `kind`, anchor, and background box. `step_id` is
/// the owning flow step's id for a `Number` badge (so it can highlight when that
/// step is selected, mirroring the edge + the sequence message); `None` for
/// structural relationship pills, which are not steps.
#[derive(Clone)]
pub struct LabelView {
    pub kind: LabelKind,
    pub step_id: Option<String>,
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub box_rect: Rect,
}

/// Render one edge label. A flow number-pill renders as a compact circular
/// badge centered on the anchor; a structural relationship label renders as a
/// small glyph pill centered on the anchor, with the relationship word as a
/// hover `title` (the word is also spelled out in the legend).
#[component]
pub fn DiagramLabel(label: LabelView, #[prop(into)] selected: Signal<bool>) -> impl IntoView {
    match label.kind {
        // The number badge takes the `--accent` active treatment when its step is
        // selected — the same STATE signal the active edge and the sequence
        // message use, so selecting a step lights up its pill on every diagram.
        LabelKind::Number(text) => view! {
            <g class="flow-label" class=("flow-label--active", move || selected.get())>
                <circle
                    class="flow-label__badge"
                    cx=label.anchor_x
                    cy=label.anchor_y
                    r=BADGE_RADIUS
                ></circle>
                <text class="flow-label__badge-num" x=label.anchor_x y=label.anchor_y>
                    {text}
                </text>
            </g>
        },
        LabelKind::Relationship { kind, word } => {
            let half = REL_PILL_SIZE / 2.0;
            let pill_x = label.anchor_x - half;
            let pill_y = label.anchor_y - half;
            let glyph_scale = (REL_PILL_SIZE - REL_GLYPH_PAD * 2.0) / 24.0;
            let glyph_transform = format!(
                "translate({} {}) scale({glyph_scale})",
                pill_x + REL_GLYPH_PAD,
                pill_y + REL_GLYPH_PAD,
            );
            view! {
                <g class="flow-label flow-label--rel">
                    // The word is available as a native SVG hover tooltip.
                    <title>{word}</title>
                    <rect
                        class="flow-label__rel-pill"
                        x=pill_x y=pill_y
                        width=REL_PILL_SIZE height=REL_PILL_SIZE
                        rx="4" ry="4"
                    ></rect>
                    <g class="flow-label__rel-glyph" transform=glyph_transform>
                        <path d=kind.icon_path()></path>
                    </g>
                </g>
            }
        }
    }
}
