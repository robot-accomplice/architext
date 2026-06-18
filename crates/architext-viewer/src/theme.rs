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
