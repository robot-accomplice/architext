//! Relationship-kind semantic line glyphs + the reusable `RelationshipIcon`.
//!
//! Structural (C4 / deployment) edges are labelled by the relationship between
//! their endpoints (`relationship_label` in the routing crate produces words
//! like `"depends on"`, `"reads/writes"`, `"uses"`). Rendered as WORDS those
//! pills collide into an unreadable mess on dense diagrams, so each label is
//! classified into a small set of relationship KINDS, and each kind renders as
//! one 24×24 line glyph (single `<path>`, `stroke=currentColor`, no fill)
//! consistent with the node-type / mode icon vocabulary. The original word is
//! preserved as the pill's hover `title` and spelled out in the legend.
//!
//! FLOW edges keep their NUMERIC step pill — this module is structural-only.

use leptos::*;

/// The semantic kind of a structural relationship, derived from its label.
/// A small closed set so the legend stays compact and the glyphs stay legible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum RelationshipKind {
    /// A dependency / call / generic "X depends on / calls / requests Y" edge.
    DependsOn,
    /// A read/write data relationship (a data-store target).
    ReadsWrites,
    /// A "uses" relationship (an actor using a system, or "uses provider/...").
    Uses,
    /// A publish-to-queue relationship.
    Publishes,
    /// Anything else (e.g. the `"relates to"` missing-endpoint fallback) — a
    /// generic link.
    RelatesTo,
}

impl RelationshipKind {
    /// Classify a structural relationship label into a kind.
    ///
    /// The label vocabulary is the output of the routing crate's
    /// `relationship_label` (target-specific phrases + type/source phrases +
    /// the `"depends on"` default + the `"relates to"` missing-endpoint
    /// fallback). The match is on the concrete phrases that rule can emit; any
    /// unrecognized phrase classifies as `RelatesTo` (a generic link) rather
    /// than guessing.
    pub fn classify(label: &str) -> Self {
        match label {
            // Data read/write.
            "reads/writes" | "reads config" | "retrieves memory" => RelationshipKind::ReadsWrites,
            // Queue publish.
            "publishes" => RelationshipKind::Publishes,
            // "uses"-family (actor uses system, external provider, MCP/skills,
            // websocket/runtime/channel — all "makes use of X").
            "uses"
            | "uses provider"
            | "uses MCP/tools"
            | "uses skills/plugins"
            | "uses websocket/control"
            | "uses runtime"
            | "uses channel" => RelationshipKind::Uses,
            // Generic missing-endpoint fallback.
            "relates to" => RelationshipKind::RelatesTo,
            // Everything else the rule emits — "depends on", "calls",
            // "calls API", "requests model", "runs turn", "schedules work",
            // "records telemetry" — is a dependency/call edge.
            _ => RelationshipKind::DependsOn,
        }
    }

    /// The SVG `path` `d` for this kind's glyph (24×24, line, currentColor).
    pub fn icon_path(self) -> &'static str {
        match self {
            // depends on: a forward line arrow (→).
            RelationshipKind::DependsOn => "M4 12h14 M14 7l5 5-5 5",
            // reads/writes: two-way vertical exchange (⇅) — read up, write down.
            RelationshipKind::ReadsWrites => "M9 4v16 M6 7l3-3 3 3 M15 4v16 M12 17l3 3 3-3",
            // uses: a hand/plug-into link — a connecting line into a target box.
            RelationshipKind::Uses => "M4 12h7 M8 9l3 3-3 3 M14 6h6v12h-6",
            // publishes: an outbound arrow into a stack (queue).
            RelationshipKind::Publishes => "M3 12h9 M9 9l3 3-3 3 M16 5v14 M20 5v14",
            // relates to: a generic undirected link (two nodes joined).
            RelationshipKind::RelatesTo => "M7 12a2.5 2.5 0 1 0 0 .01 M17 12a2.5 2.5 0 1 0 0 .01 M9.5 12h5",
        }
    }

    /// The canonical word for this kind, for the legend.
    pub fn word(self) -> &'static str {
        match self {
            RelationshipKind::DependsOn => "depends on",
            RelationshipKind::ReadsWrites => "reads/writes",
            RelationshipKind::Uses => "uses",
            RelationshipKind::Publishes => "publishes",
            RelationshipKind::RelatesTo => "relates to",
        }
    }
}

/// Renders a relationship kind's glyph as a 24×24 line SVG. Monochrome
/// (`currentColor`) so it inherits the pill's edge tone.
#[component]
pub fn RelationshipIcon(kind: RelationshipKind) -> impl IntoView {
    view! {
        <svg class="rel-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d=kind.icon_path()/>
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::RelationshipKind::*;
    use super::*;

    #[test]
    fn classifies_the_four_prompt_labels() {
        assert_eq!(RelationshipKind::classify("depends on"), DependsOn);
        assert_eq!(RelationshipKind::classify("reads/writes"), ReadsWrites);
        assert_eq!(RelationshipKind::classify("uses"), Uses);
        assert_eq!(RelationshipKind::classify("relates to"), RelatesTo);
    }

    #[test]
    fn classifies_the_full_relationship_label_vocabulary() {
        // Target-type phrases.
        assert_eq!(RelationshipKind::classify("publishes"), Publishes);
        assert_eq!(RelationshipKind::classify("uses provider"), Uses);
        // Source-type phrases — "calls" is a dependency/call edge.
        assert_eq!(RelationshipKind::classify("calls"), DependsOn);
        // Target-specific phrases from the JS table.
        assert_eq!(RelationshipKind::classify("calls API"), DependsOn);
        assert_eq!(RelationshipKind::classify("reads config"), ReadsWrites);
        assert_eq!(RelationshipKind::classify("retrieves memory"), ReadsWrites);
        assert_eq!(RelationshipKind::classify("uses MCP/tools"), Uses);
        assert_eq!(RelationshipKind::classify("uses skills/plugins"), Uses);
        assert_eq!(RelationshipKind::classify("requests model"), DependsOn);
    }

    #[test]
    fn unknown_label_classifies_as_dependency() {
        // An unrecognized non-"relates to" phrase defaults to the dependency
        // arrow, matching the rule's own "depends on" default.
        assert_eq!(RelationshipKind::classify("some new phrase"), DependsOn);
    }

    #[test]
    fn every_kind_has_a_glyph_and_a_word() {
        for kind in [DependsOn, ReadsWrites, Uses, Publishes, RelatesTo] {
            assert!(!kind.icon_path().is_empty(), "{kind:?} must have a glyph");
            assert!(!kind.word().is_empty(), "{kind:?} must have a word");
        }
    }
}
