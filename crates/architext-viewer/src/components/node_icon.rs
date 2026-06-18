//! Node-type semantic line glyphs + the reusable `NodeTypeIcon` component.
//!
//! Each C4 node `type` maps to one 24×24 line glyph (single `<path>`,
//! `stroke=currentColor`, no fill) drawn from the old viewer's `DiagramIcon`
//! vocabulary (`viewer/src/presentation/DiagramIcon.tsx`) via the same
//! `type → icon` table the JS used (`diagramIconModel.js`). The glyph inherits
//! `currentColor`, so the caller tints it to the node's `--c4-*` role color —
//! the SAME single-source token as the card's 2px top-bar (DESIGN.md rule 1:
//! the role hue encodes TYPE; it is not a state treatment).

use leptos::*;

/// The SVG `path` `d` for a node type's corner glyph.
///
/// Maps the authored (verbose) node `type` to a `DiagramIcon` glyph path,
/// mirroring the JS `nodeTypeIcons` table. Already-normalized suffixes
/// (`data`, `deployment`, `system`, `external`) also resolve so callers may
/// pass either form. Unknown types fall back to the neutral `node` box glyph
/// (the JS `?? "node"` default).
pub fn node_icon_path(node_type: &str) -> &'static str {
    match node_type {
        // actor: head + body + arms + stance (DiagramIcon `actor`).
        "actor" => "M12 5a3 3 0 1 0 0.1 0 M12 8v9 M8 12h8 M9 21l3-4 3 4",
        // software-system: framed bands (DiagramIcon `system`).
        "software-system" | "system" => "M4 6h16v12H4z M7 9h10 M7 13h10",
        // client: screen on a stand (DiagramIcon `client`).
        "client" => "M4 5h16v11H4z M9 20h6 M12 16v4",
        // service: stacked tiers (DiagramIcon `service`).
        "service" => "M5 5h14v14H5z M8 9h8 M8 13h8 M8 17h4",
        // worker: gear/cog (DiagramIcon `worker`).
        "worker" => "M12 7v-3 M12 20v-3 M7 12H4 M20 12h-3 M8.5 8.5L6.3 6.3 M17.7 17.7l-2.2-2.2 M15.5 8.5l2.2-2.2 M6.3 17.7l2.2-2.2 M9 12a3 3 0 1 0 6 0 3 3 0 0 0-6 0",
        // queue: stacked lanes with ticks (DiagramIcon `queue`).
        "queue" => "M5 6h14 M5 12h14 M5 18h14 M8 4v4 M8 10v4 M8 16v4",
        // data-store: cylinder (DiagramIcon `database`).
        "data-store" | "data" => "M6 6c0-2 12-2 12 0v12c0 2-12 2-12 0z M6 6c0 2 12 2 12 0 M6 12c0 2 12 2 12 0",
        // external-service: globe (DiagramIcon `external`).
        "external-service" | "external" => "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18 M3 12h18 M12 3c3 3 3 15 0 18 M12 3c-3 3-3 15 0 18",
        // module: nested/offset boxes (DiagramIcon `module`).
        "module" => "M4 7h16v12H4z M4 7l4-4h12v12l-4 4 M20 3v12",
        // deployment-unit: cube/parcel (DiagramIcon `package`).
        "deployment-unit" | "deployment" => "M4 8l8-4 8 4v8l-8 4-8-4z M4 8l8 4 8-4 M12 12v8",
        // trust-boundary: shield (DiagramIcon `shield`).
        "trust-boundary" => "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z",
        // Unknown → neutral node box (JS `?? "node"`).
        _ => "M5 5h14v14H5z",
    }
}

/// A human-readable label for a node type, for the legend + `aria-label`.
/// Mirrors the JS `iconLabel` words, keyed by the authored node type. An
/// unrecognized type returns its own string (no invented word).
pub fn node_type_label(node_type: &str) -> String {
    let known = match node_type {
        "actor" => "Actor",
        "software-system" | "system" => "Software system",
        "client" => "Client",
        "service" => "Service",
        "worker" => "Worker",
        "queue" => "Queue",
        "data-store" | "data" => "Data store",
        "external-service" | "external" => "External service",
        "module" => "Module",
        "deployment-unit" | "deployment" => "Deployment unit",
        "trust-boundary" => "Trust boundary",
        other => return other.to_string(),
    };
    known.to_string()
}

/// Renders a node type's glyph as a 24×24 line SVG. Monochrome
/// (`currentColor`) so the host tints it to the node's `--c4-*` role color.
#[component]
pub fn NodeTypeIcon(#[prop(into)] node_type: String) -> impl IntoView {
    let label = node_type_label(&node_type);
    view! {
        <svg class="node-icon" viewBox="0 0 24 24" aria-label=label role="img">
            <path d=node_icon_path(&node_type)/>
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_maps_to_diagram_icon_glyph() {
        // Authored (verbose) types map to their DiagramIcon glyph, per the JS
        // `nodeTypeIcons` table.
        assert_eq!(node_icon_path("actor"), "M12 5a3 3 0 1 0 0.1 0 M12 8v9 M8 12h8 M9 21l3-4 3 4");
        assert_eq!(node_icon_path("client"), "M4 5h16v11H4z M9 20h6 M12 16v4");
        assert_eq!(node_icon_path("service"), "M5 5h14v14H5z M8 9h8 M8 13h8 M8 17h4");
        // data-store → the database (cylinder) glyph, NOT a box.
        assert_eq!(
            node_icon_path("data-store"),
            "M6 6c0-2 12-2 12 0v12c0 2-12 2-12 0z M6 6c0 2 12 2 12 0 M6 12c0 2 12 2 12 0"
        );
        // software-system → the `system` glyph.
        assert_eq!(node_icon_path("software-system"), "M4 6h16v12H4z M7 9h10 M7 13h10");
        // deployment-unit → the `package` cube glyph.
        assert_eq!(node_icon_path("deployment-unit"), "M4 8l8-4 8 4v8l-8 4-8-4z M4 8l8 4 8-4 M12 12v8");
    }

    #[test]
    fn normalized_suffixes_resolve_to_the_same_glyph() {
        // Callers may pass the normalized suffix; it resolves to the same glyph.
        assert_eq!(node_icon_path("data"), node_icon_path("data-store"));
        assert_eq!(node_icon_path("system"), node_icon_path("software-system"));
        assert_eq!(node_icon_path("deployment"), node_icon_path("deployment-unit"));
        assert_eq!(node_icon_path("external"), node_icon_path("external-service"));
    }

    #[test]
    fn unknown_type_falls_back_to_the_node_box_glyph() {
        assert_eq!(node_icon_path("mystery"), "M5 5h14v14H5z");
        assert_eq!(node_icon_path(""), "M5 5h14v14H5z");
    }

    #[test]
    fn labels_match_the_authored_type_words() {
        assert_eq!(node_type_label("data-store"), "Data store");
        assert_eq!(node_type_label("software-system"), "Software system");
        assert_eq!(node_type_label("deployment-unit"), "Deployment unit");
        // Unknown type returns the raw string (no invented word).
        assert_eq!(node_type_label("mystery"), "mystery");
    }
}
