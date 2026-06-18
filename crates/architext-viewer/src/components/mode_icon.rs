//! Per-mode semantic line icons + the reusable `ModeIcon` SVG component.
//!
//! Each of the nine modes gets one 24×24 line glyph (single `<path>`,
//! `stroke=currentColor`, no fill) consistent with the old viewer's
//! `DiagramIcon` vocabulary. Several paths are reused verbatim from that icon
//! set (noted per arm); the rest are clean new line glyphs in the same style.
//! The icon inherits `currentColor`, so it tints with the row/button state —
//! state is expressed via `--accent` only (DESIGN.md rule 1), never a role hue.
use leptos::*;

use crate::theme::Mode;

/// The SVG `path` `d` for a mode's semantic icon.
///
/// Reused from `DiagramIcon` where the glyph fits; new line paths otherwise.
pub fn mode_icon_path(mode: Mode) -> &'static str {
    match mode {
        // New: two source nodes branching into one node + arrowhead (flow/branch).
        Mode::Flows => "M5 6h4 M5 18h4 M9 6c4 0 2 6 6 6 M9 18c4 0 2-6 6-6 M15 12h4 M16 9l3 3-3 3",
        // New: two lifelines (head + dashed-implied stem) with a message arrow.
        Mode::Sequence => "M7 4v16 M17 4v16 M7 10h10 M14 7l3 3-3 3",
        // Reused `module`: nested/offset boxes (C4 containment).
        Mode::C4 => "M4 7h16v12H4z M4 7l4-4h12v12l-4 4 M20 3v12",
        // Reused `package`: cube/parcel (deployment unit).
        Mode::Deployment => "M4 8l8-4 8 4v8l-8 4-8-4z M4 8l8 4 8-4 M12 12v8",
        // Reused `shield`: data classification / risk posture.
        Mode::DataRisks => "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z",
        // Reused `folder`: repository tree root.
        Mode::RepoTree => "M4 18h16V8h-9l-2-2H4z",
        // New: concentric rings + center (target / blast radius).
        Mode::BlastRadius => "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16 M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M12 11.5a.5.5 0 1 0 0 1 .5.5 0 0 0 0-1",
        // New: milestone tag/flag with eyelet (release marker).
        Mode::ReleaseTruth => "M4 4h9l7 8-7 8H4z M8 8a1.2 1.2 0 1 0 0 .01",
        // New: checklist — list rows with leading checks (project rules).
        Mode::Rules => "M4 6l1.5 1.5L8 4 M4 13l1.5 1.5L8 11 M11 6h9 M11 13h9 M11 19h6",
    }
}

/// Renders a mode's semantic icon as a 24×24 line SVG. Monochrome
/// (`currentColor`) so it tints with the host element's state.
#[component]
pub fn ModeIcon(mode: Mode) -> impl IntoView {
    view! {
        <svg class="mode-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d=mode_icon_path(mode)/>
        </svg>
    }
}
