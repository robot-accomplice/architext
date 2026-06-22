//! Edge renderer (one routed edge per call).
//!
//! The `d`-string from the routing engine is the byte-parity-proven geometry —
//! it is used verbatim. The edge kind (process / decision / async / return)
//! selects a CSS class for the DESIGN.md treatment (e.g. dashed async). The
//! arrowhead is a shared `<marker>` defined once in `svg.rs`.

use leptos::*;

use super::EdgeKind;

/// An edge ready to render: the verbatim `d`-string plus the resolved kind.
#[derive(Clone)]
pub struct EdgeView {
    pub id: String,
    pub d: String,
    pub kind: EdgeKind,
}

/// The shared arrowhead marker id (defined once in the `<defs>`).
pub const ARROWHEAD_ID: &str = "flow-arrowhead";

/// Render one routed edge. The path geometry is the engine's `d` verbatim; the
/// kind class drives stroke styling; every edge ends in the shared arrowhead.
///
/// `selected` (the steps-panel selection, keyed route id == step id) adds the
/// `flow-edge--active` STATE class — a brighter/thicker `--accent` stroke (rule
/// 1: state is `--accent`, never a role hue). The route `d` is untouched.
#[component]
pub fn DiagramEdge(
    edge: EdgeView,
    #[prop(into)] selected: Signal<bool>,
) -> impl IntoView {
    let base = format!("flow-edge {}", edge.kind.css_class());
    let class = move || {
        if selected.get() {
            format!("{base} flow-edge--active")
        } else {
            base.clone()
        }
    };
    // A stem is a tether, not a directional message → no arrowhead.
    let marker_end = if edge.kind.has_arrowhead() {
        format!("url(#{ARROWHEAD_ID})")
    } else {
        String::new()
    };
    view! {
        <path
            class=class
            d=edge.d
            fill="none"
            marker-end=marker_end
        ></path>
    }
}
