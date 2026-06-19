//! Theme / token surface for the viewer chrome.
//!
//! CSS owns the actual values (`styles.css` `:root`, derived from
//! `viewer/DESIGN.md`). This module is the Rust-side single source for the
//! *enumerated* design facts the components need to render — currently the nine
//! navigation modes. Keeping them here (not as literals scattered across
//! components) follows the workspace's "no magic literals" convention and gives
//! later slices one place to attach per-mode data wiring.

/// The nine viewer modes, in nav order (DESIGN.md "one product, not nine").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Flows,
    Sequence,
    C4,
    Deployment,
    DataRisks,
    RepoTree,
    BlastRadius,
    ReleaseTruth,
    Rules,
}

impl Mode {
    /// Nav order, rendered as the left-nav mode list.
    pub const ALL: [Mode; 9] = [
        Mode::Flows,
        Mode::Sequence,
        Mode::C4,
        Mode::Deployment,
        Mode::DataRisks,
        Mode::RepoTree,
        Mode::BlastRadius,
        Mode::ReleaseTruth,
        Mode::Rules,
    ];

    /// Human label shown in the nav.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Flows => "Flows",
            Mode::Sequence => "Sequence",
            Mode::C4 => "C4",
            Mode::Deployment => "Deployment",
            Mode::DataRisks => "Data / Risks",
            Mode::RepoTree => "Repo Tree",
            Mode::BlastRadius => "Blast Radius",
            Mode::ReleaseTruth => "Release Truth",
            Mode::Rules => "Rules",
        }
    }

    /// The JS/data mode id (matches `MODE_DEFINITIONS` ids), used to drive the
    /// ported `architext_routing::plan_request::view_selection` logic.
    pub fn id(self) -> &'static str {
        match self {
            Mode::Flows => "flows",
            Mode::Sequence => "sequence",
            Mode::C4 => "c4",
            Mode::Deployment => "deployment",
            Mode::DataRisks => "data-risks",
            Mode::RepoTree => "repo-tree",
            Mode::BlastRadius => "blast-radius",
            Mode::ReleaseTruth => "release-truth",
            Mode::Rules => "rules",
        }
    }

    /// Whether this is the Flows mode specifically (the flow drives the view and
    /// the view selector offers every compatible flow projection).
    pub fn is_flows(self) -> bool {
        matches!(self, Mode::Flows)
    }

    /// A neutral one-line description of what a *diagram-less* list mode shows,
    /// surfaced in the selector rail in place of a diagram/flow selector. Replaces
    /// the old terse, negative "X has no diagram projection" note (which read as
    /// an error) with a positive statement of the mode's content. Returns `None`
    /// for modes that drive a diagram or carry their own selector (Release Truth),
    /// where the rail already has an affordance and no note is shown.
    pub fn rail_summary(self) -> Option<&'static str> {
        match self {
            Mode::RepoTree => Some("Browse the repository file tree with ownership tags."),
            Mode::BlastRadius => Some("Trace what a change to a node reaches across the model."),
            Mode::Rules => Some("Review the project's rules, ranked by criticality."),
            _ => None,
        }
    }

    /// Whether this mode renders one selected flow as a ROUTED `plan()` diagram
    /// (flow drives → view resolves to a compatible flow-projection → the shared
    /// `DiagramSvg` renders it). Both Flows and Data/Risks do this; Data/Risks
    /// adds the data-class/risk side panel over the same diagram path. (Sequence
    /// also renders a flow, but as lifelines, not a routed plan — see
    /// [`Self::projects_flows`].)
    pub fn renders_routed_flow(self) -> bool {
        matches!(self, Mode::Flows | Mode::DataRisks)
    }

    /// Whether this mode is driven by a selected FLOW (so the UI shows a flow
    /// selector and the state seeds/​resolves a flow). The Flows and Data/Risks
    /// routed-plan projections and the Sequence lifeline projection all render
    /// one selected flow; the difference is how each lays it out, handled
    /// downstream.
    pub fn projects_flows(self) -> bool {
        matches!(self, Mode::Flows | Mode::Sequence | Mode::DataRisks)
    }
}

/// Color theme — dark (the locked Cyber-Tactical default) or light. The actual
/// token values live in `styles.css` (`:root` = dark, `:root[data-theme=light]`
/// = light); this enum is the Rust-side state + the `data-theme` attribute value
/// applied to `<html>`. Persisted in localStorage so the choice survives reload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Dark,
    Light,
}

const THEME_STORAGE_KEY: &str = "architext-theme";

impl Theme {
    /// The `data-theme` attribute value (also the persisted string).
    pub fn attr(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    fn from_attr(s: &str) -> Option<Self> {
        match s {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        }
    }

    /// The other theme (for a toggle).
    pub fn toggled(self) -> Self {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        }
    }

    /// Label for the toggle control — names the theme it switches TO.
    pub fn toggle_label(self) -> &'static str {
        match self {
            Theme::Dark => "Light",
            Theme::Light => "Dark",
        }
    }

    /// Glyph for the toggle — shows the target theme's icon (sun when going to
    /// light, moon when going to dark).
    pub fn toggle_icon(self) -> &'static str {
        match self {
            Theme::Dark => "☀",
            Theme::Light => "☾",
        }
    }
}

/// Browser `localStorage` (same-origin), or None outside a window context.
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The persisted theme, defaulting to Dark (the locked design language) when no
/// choice has been stored.
pub fn load_theme() -> Theme {
    local_storage()
        .and_then(|s| s.get_item(THEME_STORAGE_KEY).ok().flatten())
        .and_then(|v| Theme::from_attr(&v))
        .unwrap_or(Theme::Dark)
}

/// Persist the theme choice and apply it as `data-theme` on `<html>` so every
/// surface (including popovers rendered outside the app subtree) re-themes.
pub fn apply_theme(theme: Theme) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(root) = doc.document_element() {
            let _ = root.set_attribute("data-theme", theme.attr());
        }
    }
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(THEME_STORAGE_KEY, theme.attr());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_modes_get_a_neutral_rail_summary_others_get_none() {
        // The three diagram-less LIST modes describe what they show (no negative
        // "has no diagram projection" messaging); every diagram/selector-driven
        // mode returns None so the rail keeps its own affordance.
        for mode in [Mode::RepoTree, Mode::BlastRadius, Mode::Rules] {
            let summary = mode.rail_summary().expect("list mode should have a summary");
            assert!(!summary.is_empty());
            assert!(
                !summary.to_lowercase().contains("no diagram"),
                "{} summary must not carry negative 'no diagram' wording: {summary}",
                mode.label(),
            );
        }
        for mode in [
            Mode::Flows,
            Mode::Sequence,
            Mode::C4,
            Mode::Deployment,
            Mode::DataRisks,
            Mode::ReleaseTruth,
        ] {
            assert!(mode.rail_summary().is_none(), "{} should have no rail summary", mode.label());
        }
    }
}
