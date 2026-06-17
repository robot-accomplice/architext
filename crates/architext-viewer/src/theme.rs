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
}
