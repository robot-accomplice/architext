//! Flows-mode diagram canvas.
//!
//! Clean split (component-per-file, no god-component):
//! - `plan`  — in-process plan compute (Leptos-free, native-testable).
//! - `svg`   — the `DiagramSvg` component: the `<svg>` + pan/zoom transform `<g>`
//!   + arrowhead marker defs; composes the node/edge/label renderers.
//! - `node`  — one node card (rect + category top-bar + name + type chip), plus
//!   decision diamonds.
//! - `edge`  — one routed edge (`<path d=…>` + arrowhead + kind treatment).
//! - `label` — one step label (background box + number/text).
//!
//! Role color is single-sourced: a node's `type` maps to the `--c4-{type}` CSS
//! token (see [`role_color_var`]). Components never emit a raw palette hue.
//! Selection is a STATE treatment (`--accent` ring/glow), never a role hue.

pub mod edge;
pub mod label;
pub mod node;
pub mod plan;
pub mod svg;

pub use svg::DiagramSvg;

/// The C4 role tokens defined in `styles.css` (`--c4-*`). A node `type` is the
/// single source for its role color; this set is the allow-list of types that
/// have a dedicated token. Unknown types fall back to `--c4-external` (the
/// neutral "outside the model" role) rather than an invented hue.
const C4_ROLES: &[&str] = &[
    "actor", "client", "service", "worker", "data", "external", "deployment", "module",
];

/// The CSS `var(...)` reference for a node type's role color.
///
/// SINGLE SOURCE: the actual hue lives only in `styles.css` `:root` as
/// `--c4-{type}`. This returns a `var()` reference — never a literal color — so
/// the diagram inherits whatever the design tokens define. Unknown types map to
/// `--c4-external`.
pub fn role_color_var(node_type: &str) -> String {
    if C4_ROLES.contains(&node_type) {
        format!("var(--c4-{node_type})")
    } else {
        "var(--c4-external)".to_string()
    }
}

/// The semantic kind of a routed edge, derived from the flow step that produced
/// it. Drives the DESIGN.md edge treatment (dashed async, decision-branch
/// outcome label, return styling). `Process` is the default solid edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    Process,
    Decision,
    Async,
    Return,
}

impl EdgeKind {
    /// Resolve from a flow step's `kind` string.
    pub fn from_step_kind(kind: Option<&str>) -> Self {
        match kind {
            Some("decision") => EdgeKind::Decision,
            Some("async") => EdgeKind::Async,
            Some("return") => EdgeKind::Return,
            _ => EdgeKind::Process,
        }
    }

    /// The CSS class applied to the edge group for kind-specific styling.
    pub fn css_class(self) -> &'static str {
        match self {
            EdgeKind::Process => "flow-edge--process",
            EdgeKind::Decision => "flow-edge--decision",
            EdgeKind::Async => "flow-edge--async",
            EdgeKind::Return => "flow-edge--return",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_color_is_a_var_reference_never_a_hue() {
        assert_eq!(role_color_var("service"), "var(--c4-service)");
        assert_eq!(role_color_var("data"), "var(--c4-data)");
        // Unknown type → neutral external role, still a var (no invented hue).
        assert_eq!(role_color_var("mystery"), "var(--c4-external)");
        // No output is ever a raw `#rrggbb`.
        for t in ["actor", "client", "worker", "module", "weird"] {
            assert!(role_color_var(t).starts_with("var(--c4-"));
        }
    }

    #[test]
    fn edge_kind_maps_step_kinds() {
        assert_eq!(EdgeKind::from_step_kind(Some("decision")), EdgeKind::Decision);
        assert_eq!(EdgeKind::from_step_kind(Some("async")), EdgeKind::Async);
        assert_eq!(EdgeKind::from_step_kind(Some("return")), EdgeKind::Return);
        assert_eq!(EdgeKind::from_step_kind(None), EdgeKind::Process);
        assert_eq!(EdgeKind::from_step_kind(Some("other")), EdgeKind::Process);
    }
}
