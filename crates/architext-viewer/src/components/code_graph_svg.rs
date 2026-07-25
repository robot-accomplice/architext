//! SVG emission for the Code Graph mode.
//!
//! Renders an already-positioned `GraphLayout` — no layout maths here (that is
//! `code_graph_model`), matching how `diagram::edge` consumes a routed `d`
//! verbatim.
//!
//! DESIGN.md rule 3: the `<svg>` is fluid (`viewBox` + `preserveAspectRatio`),
//! and pan/zoom is ONE transform on an inner `<g>` — never on the element.
//! Edges are ALWAYS solid: dynamic dispatch is signalled by a distinct
//! arrowhead marker and stroke colour, never by dashing (a sequence-diagram
//! device).
use leptos::*;

use crate::code_graph_model::{GraphEdge, GraphLayout, GraphNode};

const STATIC_ARROW_ID: &str = "cg-arrow-static";
const DYNAMIC_ARROW_ID: &str = "cg-arrow-dynamic";

/// Stroke width from the number of underlying call edges — a heavier line means
/// more real calls collapsed into one module→module edge.
fn edge_width(count: u32) -> f64 {
    (1.0 + (count as f64).log10()).clamp(1.0, 4.0)
}

#[component]
pub fn CodeGraphSvg(
    layout: GraphLayout,
    pan_x: RwSignal<f64>,
    pan_y: RwSignal<f64>,
    zoom: RwSignal<f64>,
    selected: RwSignal<Option<String>>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let transform = move || {
        format!("translate({} {}) scale({})", pan_x.get(), pan_y.get(), zoom.get())
    };
    let view_box = format!(
        "0 0 {:.0} {:.0}",
        layout.content_width.max(1.0),
        layout.content_height.max(1.0)
    );

    let edges = layout.edges.clone();
    let nodes = layout.nodes.clone();

    view! {
        <svg class="code-graph-svg" viewBox=view_box preserveAspectRatio="xMidYMid meet">
            <defs>
                <marker id=STATIC_ARROW_ID viewBox="0 0 10 10" refX="9" refY="5"
                    markerWidth="7" markerHeight="7" markerUnits="userSpaceOnUse"
                    orient="auto-start-reverse">
                    <path d="M 0 0 L 10 5 L 0 10 z" class="cg-arrowhead"></path>
                </marker>
                // Dynamic dispatch gets an OPEN chevron rather than a filled
                // head — a shape difference, so it survives greyscale and
                // colour-blind viewing where a hue difference would not.
                <marker id=DYNAMIC_ARROW_ID viewBox="0 0 10 10" refX="9" refY="5"
                    markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse"
                    orient="auto-start-reverse">
                    <path d="M 0 0 L 10 5 L 0 10" class="cg-arrowhead cg-arrowhead--dynamic"></path>
                </marker>
            </defs>
            <g class="code-graph-svg__transform" transform=transform>
                <g class="code-graph-svg__edges">
                    {edges.into_iter().map(|e| view! { <CodeGraphEdgeView edge=e/> }).collect_view()}
                </g>
                <g class="code-graph-svg__nodes">
                    {nodes.into_iter().map(|n| {
                        let id = n.id.clone();
                        let is_selected = Signal::derive(move || {
                            selected.get().as_deref() == Some(id.as_str())
                        });
                        view! { <CodeGraphNodeView node=n selected=is_selected on_select=on_select/> }
                    }).collect_view()}
                </g>
            </g>
        </svg>
    }
}

#[component]
fn CodeGraphEdgeView(edge: GraphEdge) -> impl IntoView {
    let marker = if edge.dynamic {
        format!("url(#{DYNAMIC_ARROW_ID})")
    } else {
        format!("url(#{STATIC_ARROW_ID})")
    };
    let title = if edge.dynamic {
        format!("{} call(s) · includes dynamic dispatch (RTA over-approximation)", edge.count)
    } else {
        format!("{} static call(s)", edge.count)
    };
    view! {
        <path
            class="cg-edge"
            class=("cg-edge--dynamic", edge.dynamic)
            d=edge.d
            fill="none"
            stroke-width=edge_width(edge.count)
            marker-end=marker
        >
            <title>{title}</title>
        </path>
    }
}

#[component]
fn CodeGraphNodeView(
    node: GraphNode,
    #[prop(into)] selected: Signal<bool>,
    #[prop(into)] on_select: Callback<String>,
) -> impl IntoView {
    let click_id = node.id.clone();
    let transform = format!("translate({:.1} {:.1})", node.x, node.y);
    let fan = format!("in {} · out {}", node.fan_in, node.fan_out);
    let badges = node.badges.clone();

    view! {
        <g
            class="cg-node"
            class:is-active=move || selected.get()
            class=("cg-node--drillable", node.drillable)
            transform=transform
            on:click=move |_| on_select.call(click_id.clone())
        >
            <rect class="cg-node__card" x="0" y="0" width=node.w height=node.h rx="6"></rect>
            <text class="cg-node__label" x="10" y="20">{node.label.clone()}</text>
            <text class="cg-node__sublabel" x="10" y="36">{node.sublabel.clone()}</text>
            <text class="cg-node__fan" x="10" y="52">{fan}</text>
            <g class="cg-node__badges" transform=format!("translate({:.1} 12)", node.w - 10.0)>
                {badges.into_iter().enumerate().map(|(i, b)| {
                    view! {
                        <circle
                            class="cg-badge"
                            cx=0
                            cy=i as f64 * 12.0
                            r="4"
                            style=format!("fill:{}", b.color_var())
                        >
                            <title>{format!("{}: {}", b.label(), b.tooltip())}</title>
                        </circle>
                    }
                }).collect_view()}
            </g>
        </g>
    }
}
