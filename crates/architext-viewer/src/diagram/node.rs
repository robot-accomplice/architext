//! Node-card and decision-diamond renderers (one node per call).
//!
//! A node card is an 8px-radius rect (`--node-card-radius`) with a 2px category
//! top-bar in the node's `--c4-{type}` role color, the node name (Hanken
//! Grotesk via `.flow-node__name`), and the type as a chip (JetBrains Mono via
//! `.flow-node__type`). The SELECTED node gets the `--accent` state treatment
//! (ring/glow) — a STATE class, never a role hue.

use architext_routing::model::Rect;
use leptos::*;

use super::role_color_var;
use crate::components::node_icon::node_icon_path;

/// Corner type-glyph geometry (canvas px). The 24×24 line glyph is scaled to
/// `ICON_SIZE` and inset from the card's top-left by `ICON_PAD` (clear of the
/// 2px top-bar and the centered name).
const ICON_SIZE: f64 = 15.0;
const ICON_PAD: f64 = 5.0;

/// A node ready to render: its placed rect plus the display fields resolved
/// from the loaded dataset (name + C4 type).
#[derive(Clone)]
pub struct NodeView {
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub rect: Rect,
}

/// Render one node card. `selected` toggles the `--accent` state treatment;
/// `on_select` fires the node id on click so the inspector can bind to it.
#[component]
pub fn DiagramNode(
    node: NodeView,
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let Rect { x, y, width, height } = node.rect;
    let role = role_color_var(&node.node_type);
    let id_for_click = node.id.clone();
    // Corner type-glyph: the node's type → DiagramIcon glyph, tinted to the
    // SAME `--c4-*` role token as the top-bar (single source). The 24×24 path
    // is scaled to ICON_SIZE and positioned just below the top-bar at the left.
    let icon_d = node_icon_path(&node.node_type);
    let icon_scale = ICON_SIZE / 24.0;
    let icon_transform = format!("translate({ICON_PAD} {ICON_PAD}) scale({icon_scale})");
    let icon_color = role.clone();

    // Card classes: base + selected state (rule 1: state ≠ role hue).
    let group_class = move || {
        if selected.get() {
            "flow-node flow-node--selected"
        } else {
            "flow-node"
        }
    };

    view! {
        <g
            class=group_class
            transform=format!("translate({x} {y})")
            on:click=move |_| on_select.call(id_for_click.clone())
        >
            // Card body — 8px radius via --node-card-radius (set in CSS).
            <rect class="flow-node__card" width=width height=height rx="8" ry="8"></rect>
            // 2px category top-bar in the node's role color (single source).
            <rect class="flow-node__topbar" width=width height="2" fill=role.clone()></rect>
            // Upper-left type glyph, tinted to the SAME role token as the bar.
            <g class="flow-node__icon" transform=icon_transform style=("color", icon_color)>
                <path d=icon_d></path>
            </g>
            // Node name (Hanken Grotesk).
            <text class="flow-node__name" x=width / 2.0 y=height / 2.0 - 4.0>
                {node.name.clone()}
            </text>
            // Type chip (JetBrains Mono), tinted with the role color.
            <text class="flow-node__type" x=width / 2.0 y=height / 2.0 + 14.0 fill=role>
                {node.node_type.clone()}
            </text>
        </g>
    }
}

/// Render a decision diamond (an extra decision-node rect, `fixedPorts`). It is
/// a rotated square centered on the rect; tinted with the affiliated component's
/// role color so the branch point reads as part of that node's lane.
#[component]
pub fn DecisionDiamond(rect: Rect, #[prop(into)] role_var: String) -> impl IntoView {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let r = rect.width / 2.0;
    // Diamond points: top, right, bottom, left.
    let points = format!(
        "{cx},{} {},{cy} {cx},{} {},{cy}",
        cy - r,
        cx + r,
        cy + r,
        cx - r,
    );
    view! {
        <polygon class="flow-decision" points=points stroke=role_var></polygon>
    }
}
