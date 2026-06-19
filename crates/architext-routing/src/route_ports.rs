//! Faithful port of `viewer/src/routing/routePorts.js`.
//!
//! Translation decisions:
//! - `Math.floor` → `f64::floor` (exact for finite values).
//! - `Math.round` on offsets in `portCandidatesFor` → `js_compat::js_round`.
//! - `clamp` imported from `crate::route_geometry`.
//! - `MIN_LEGIBLE_GAP` imported from `crate::route_constants`.
//! - `new Set(…)` in `portCandidatesFor` → `IndexSet` to preserve insertion
//!   order while deduplicating, matching JS `Set` semantics exactly.
//!   Deduplication is on the rounded offset value (f64 bits, with -0 canonicalized
//!   to 0 to match JS SameValueZero).
//! - The `sideAnchors` optional field on `Rect` is not present in
//!   `crate::model::Rect`; we handle it via an explicit `RectWithAnchors` type
//!   that callers can opt into, or via the plain `Rect` path. However, routePorts
//!   is called from higher-level modules that pass a generic rect; we represent
//!   the optional override via a separate function signature so the common path
//!   (no side-anchors) keeps `&Rect` and the override goes through
//!   `anchor_for_with_overrides`.

use crate::js_compat::js_round;
use crate::model::{Point, Rect};
use crate::route_constants::MIN_LEGIBLE_GAP;
use crate::route_geometry::clamp;
use indexmap::IndexSet;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Ordered array of side names, matching JS `SIDES`.
pub const SIDES: [&str; 4] = ["left", "right", "top", "bottom"];

/// Port stub length: how far from the node surface the port protrudes.
pub const PORT_STUB: f64 = 18.0;

/// Internal per-port spacing band (6 px). Used by `offsetForEndpointOrder`.
pub const PORT_SPACING: f64 = 6.0;

/// Minimum legible gap between surface mount points. Equals `MIN_LEGIBLE_GAP`
/// (4 px) — same physical contract, avoiding a second magic number.
pub const SURFACE_PORT_SPACING: f64 = MIN_LEGIBLE_GAP;

// ---------------------------------------------------------------------------
// Side anchor override map (mirrors JS `rect.sideAnchors?.[side]`)
// ---------------------------------------------------------------------------

/// Optional per-side anchor overrides. When a caller provides these, the
/// canonical anchor for that side is replaced entirely. Mirrors JS
/// `rect.sideAnchors?.[side]`.
#[derive(Debug, Clone, Default)]
pub struct SideAnchors {
    pub left: Option<Point>,
    pub right: Option<Point>,
    pub top: Option<Point>,
    pub bottom: Option<Point>,
}

impl SideAnchors {
    pub fn get(&self, side: &str) -> Option<&Point> {
        match side {
            "left" => self.left.as_ref(),
            "right" => self.right.as_ref(),
            "top" => self.top.as_ref(),
            "bottom" => self.bottom.as_ref(),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// anchorFor
// ---------------------------------------------------------------------------

/// Port of JS `anchorFor(rect, side)`.
///
/// Returns the geometric center of `side` unless a `side_anchors` override is
/// provided for that side. The `fixed_ports` flag has no effect on anchor
/// computation; it affects offset clamping in `portFor`.
pub fn anchor_for(rect: &Rect, side: &str) -> Point {
    anchor_for_with_overrides(rect, side, None)
}

/// `anchorFor` with optional `SideAnchors` override (JS `rect.sideAnchors?.[side]`).
pub fn anchor_for_with_overrides(
    rect: &Rect,
    side: &str,
    side_anchors: Option<&SideAnchors>,
) -> Point {
    if let Some(anchors) = side_anchors {
        if let Some(p) = anchors.get(side) {
            return p.clone();
        }
    }
    match side {
        "left" => Point { x: rect.x, y: rect.y + rect.height / 2.0 },
        "right" => Point { x: rect.x + rect.width, y: rect.y + rect.height / 2.0 },
        "top" => Point { x: rect.x + rect.width / 2.0, y: rect.y },
        _ => Point { x: rect.x + rect.width / 2.0, y: rect.y + rect.height }, // "bottom"
    }
}

// ---------------------------------------------------------------------------
// sideVector
// ---------------------------------------------------------------------------

/// Port of JS `sideVector(side)`.
///
/// Returns the unit outward-normal vector for `side`.
pub fn side_vector(side: &str) -> Point {
    match side {
        "left" => Point { x: -1.0, y: 0.0 },
        "right" => Point { x: 1.0, y: 0.0 },
        "top" => Point { x: 0.0, y: -1.0 },
        _ => Point { x: 0.0, y: 1.0 }, // "bottom"
    }
}

// ---------------------------------------------------------------------------
// tangentVector
// ---------------------------------------------------------------------------

/// Port of JS `tangentVector(side)`.
///
/// Returns the tangent (along-surface) unit vector for `side`.
/// Left/right sides have a vertical tangent; top/bottom have a horizontal one.
pub fn tangent_vector(side: &str) -> Point {
    if side == "left" || side == "right" {
        Point { x: 0.0, y: 1.0 }
    } else {
        Point { x: 1.0, y: 0.0 } // "top" | "bottom"
    }
}

// ---------------------------------------------------------------------------
// offsetForEndpointOrder
// ---------------------------------------------------------------------------

/// Port of JS `offsetForEndpointOrder(order)`.
///
/// Maps a linear `order` index to a lane/band offset in pixels.
/// `lane = order % 7`, `band = floor(order / 7)`.
pub fn offset_for_endpoint_order(order: u32) -> f64 {
    let lane = (order % 7) as f64;
    let band = f64::floor(order as f64 / 7.0);
    (lane - 3.0) * PORT_SPACING + band * PORT_SPACING * 7.0
}

// ---------------------------------------------------------------------------
// surfaceCapacity
// ---------------------------------------------------------------------------

/// Port of JS `surfaceCapacity(rect, side)`.
///
/// Maximum number of mounts that fit while holding at least `SURFACE_PORT_SPACING`
/// between each pair. Returns at least 1.
pub fn surface_capacity(rect: &Rect, side: &str) -> u32 {
    let length = if side == "left" || side == "right" {
        rect.height
    } else {
        rect.width
    };
    f64::max(1.0, f64::floor(length / SURFACE_PORT_SPACING) - 1.0) as u32
}

// ---------------------------------------------------------------------------
// portFor
// ---------------------------------------------------------------------------

/// Port result — the anchor (surface point, possibly offset) and the actual
/// port point (stub distance out from the surface).
#[derive(Debug, Clone, PartialEq)]
pub struct PortResult {
    pub anchor: Point,
    pub port: Point,
}

/// Port of JS `portFor(rect, side, distance = PORT_STUB, rawOffset = 0)`.
///
/// Computes the port position for a given side, optional stub distance, and
/// raw tangential offset. When `fixed_ports` is true, the offset is always 0.
pub fn port_for(
    rect: &Rect,
    side: &str,
    distance: f64,
    raw_offset: f64,
    fixed_ports: bool,
) -> PortResult {
    port_for_with_anchors(rect, side, distance, raw_offset, fixed_ports, None)
}

/// `portFor` with optional `SideAnchors` override.
pub fn port_for_with_anchors(
    rect: &Rect,
    side: &str,
    distance: f64,
    raw_offset: f64,
    fixed_ports: bool,
    side_anchors: Option<&SideAnchors>,
) -> PortResult {
    let anchor = anchor_for_with_overrides(rect, side, side_anchors);
    let vector = side_vector(side);
    let max_offset = if fixed_ports {
        0.0
    } else if side == "left" || side == "right" {
        rect.height / 2.0 - 8.0
    } else {
        rect.width / 2.0 - 8.0
    };
    let offset = if fixed_ports {
        0.0
    } else {
        clamp(raw_offset, -max_offset, max_offset)
    };
    let tangent = tangent_vector(side);
    let offset_anchor = Point {
        x: anchor.x + tangent.x * offset,
        y: anchor.y + tangent.y * offset,
    };
    PortResult {
        anchor: offset_anchor.clone(),
        port: Point {
            x: offset_anchor.x + vector.x * distance,
            y: offset_anchor.y + vector.y * distance,
        },
    }
}

// ---------------------------------------------------------------------------
// portCandidatesFor
// ---------------------------------------------------------------------------

/// Port of JS `portCandidatesFor(rect, side, offsets)`.
///
/// When `fixed_ports` is true, returns a single zero-offset port (no matter
/// what offsets are given).  Otherwise rounds and deduplicates offsets (using
/// `IndexSet` for insertion-order-preserving uniqueness, matching JS `Set`),
/// clamps each to the surface boundary, and returns a `PortResult` per offset.
pub fn port_candidates_for(
    rect: &Rect,
    side: &str,
    offsets: &[f64],
    fixed_ports: bool,
) -> Vec<PortResult> {
    port_candidates_for_with_anchors(rect, side, offsets, fixed_ports, None)
}

/// `portCandidatesFor` with optional `SideAnchors` override.
pub fn port_candidates_for_with_anchors(
    rect: &Rect,
    side: &str,
    offsets: &[f64],
    fixed_ports: bool,
    side_anchors: Option<&SideAnchors>,
) -> Vec<PortResult> {
    if fixed_ports {
        return vec![port_for_with_anchors(rect, side, PORT_STUB, 0.0, true, side_anchors)];
    }
    let max_offset = if side == "left" || side == "right" {
        rect.height / 2.0 - 8.0
    } else {
        rect.width / 2.0 - 8.0
    };
    // JS: [...new Set(offsets.map(o => Math.round(clamp(o, -maxOffset, maxOffset))))]
    // We round each, then deduplicate preserving first-occurrence (insertion order).
    // JS Set uses SameValueZero: -0 === 0. Canonicalize via bits, treating -0 == 0.
    let mut seen: IndexSet<u64> = IndexSet::new();
    let unique_offsets: Vec<f64> = offsets
        .iter()
        .map(|&o| js_round(clamp(o, -max_offset, max_offset)))
        .filter(|&rounded| {
            // Canonicalize -0.0 to 0.0 for dedup key, matching JS SameValueZero.
            let key = if rounded == 0.0 { 0.0_f64 } else { rounded };
            seen.insert(key.to_bits())
        })
        .collect();
    unique_offsets
        .into_iter()
        .map(|offset| port_for_with_anchors(rect, side, PORT_STUB, offset, false, side_anchors))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rect;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    // --- constants ---

    #[test]
    fn constants_match_js() {
        // Node: PORT_STUB=18, PORT_SPACING=6, SURFACE_PORT_SPACING=4 (MIN_LEGIBLE_GAP)
        assert_eq!(PORT_STUB, 18.0);
        assert_eq!(PORT_SPACING, 6.0);
        assert_eq!(SURFACE_PORT_SPACING, 4.0);
        assert_eq!(SIDES, ["left", "right", "top", "bottom"]);
    }

    // --- anchorFor ---

    #[test]
    fn anchor_for_sides() {
        // Node: rect={x:10,y:20,w:80,h:40}
        // left: {x:10,y:40}, right: {x:90,y:40}, top: {x:50,y:20}, bottom: {x:50,y:60}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(anchor_for(&r, "left"), pt(10.0, 40.0));
        assert_eq!(anchor_for(&r, "right"), pt(90.0, 40.0));
        assert_eq!(anchor_for(&r, "top"), pt(50.0, 20.0));
        assert_eq!(anchor_for(&r, "bottom"), pt(50.0, 60.0));
    }

    #[test]
    fn anchor_for_with_side_anchor_override() {
        // Node: rectWithSide.sideAnchors.left={x:5,y:25} → {x:5,y:25}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let anchors = SideAnchors {
            left: Some(pt(5.0, 25.0)),
            ..Default::default()
        };
        assert_eq!(anchor_for_with_overrides(&r, "left", Some(&anchors)), pt(5.0, 25.0));
        // right has no override → computed normally
        assert_eq!(anchor_for_with_overrides(&r, "right", Some(&anchors)), pt(90.0, 40.0));
    }

    // --- sideVector ---

    #[test]
    fn side_vector_all_sides() {
        // Node: left={x:-1,y:0}, right={x:1,y:0}, top={x:0,y:-1}, bottom={x:0,y:1}
        assert_eq!(side_vector("left"), pt(-1.0, 0.0));
        assert_eq!(side_vector("right"), pt(1.0, 0.0));
        assert_eq!(side_vector("top"), pt(0.0, -1.0));
        assert_eq!(side_vector("bottom"), pt(0.0, 1.0));
    }

    // --- tangentVector ---

    #[test]
    fn tangent_vector_sides() {
        // Node: left/right → {x:0,y:1}; top/bottom → {x:1,y:0}
        assert_eq!(tangent_vector("left"), pt(0.0, 1.0));
        assert_eq!(tangent_vector("right"), pt(0.0, 1.0));
        assert_eq!(tangent_vector("top"), pt(1.0, 0.0));
        assert_eq!(tangent_vector("bottom"), pt(1.0, 0.0));
    }

    // --- offsetForEndpointOrder ---

    #[test]
    fn offset_for_endpoint_order_goldens() {
        // Node: order=0→-18, 1→-12, 2→-6, 3→0, 6→18, 7→24 (band=1, lane=0: (0-3)*6+1*42=24)
        // Wait: 7→ lane=0, band=1: (0-3)*6 + 1*6*7 = -18+42 = 24. Yes.
        // 8→ lane=1, band=1: (1-3)*6 + 42 = -12+42 = 30
        // 14→ lane=0, band=2: (0-3)*6 + 2*42 = -18+84 = 66
        assert_eq!(offset_for_endpoint_order(0), -18.0);
        assert_eq!(offset_for_endpoint_order(1), -12.0);
        assert_eq!(offset_for_endpoint_order(2), -6.0);
        assert_eq!(offset_for_endpoint_order(3), 0.0);
        assert_eq!(offset_for_endpoint_order(6), 18.0);
        assert_eq!(offset_for_endpoint_order(7), 24.0);
        assert_eq!(offset_for_endpoint_order(8), 30.0);
        assert_eq!(offset_for_endpoint_order(14), 66.0);
    }

    // --- surfaceCapacity ---

    #[test]
    fn surface_capacity_goldens() {
        // Node: rect h=40, side=left → floor(40/4)-1=9
        //       rect w=80, side=top  → floor(80/4)-1=19
        //       small rect h=4       → max(1, floor(4/4)-1)=max(1,0)=1
        let r = rect(10.0, 20.0, 80.0, 40.0);
        assert_eq!(surface_capacity(&r, "left"), 9);
        assert_eq!(surface_capacity(&r, "right"), 9);
        assert_eq!(surface_capacity(&r, "top"), 19);
        assert_eq!(surface_capacity(&r, "bottom"), 19);
        let small = rect(0.0, 0.0, 4.0, 4.0);
        assert_eq!(surface_capacity(&small, "left"), 1);
    }

    // --- portFor ---

    #[test]
    fn port_for_left_default() {
        // Node: portFor(rect,'left') → anchor:{x:10,y:40}, port:{x:-8,y:40}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let p = port_for(&r, "left", PORT_STUB, 0.0, false);
        assert_eq!(p.anchor, pt(10.0, 40.0));
        assert_eq!(p.port, pt(-8.0, 40.0));
    }

    #[test]
    fn port_for_right_default() {
        // Node: anchor:{x:90,y:40}, port:{x:108,y:40}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let p = port_for(&r, "right", PORT_STUB, 0.0, false);
        assert_eq!(p.anchor, pt(90.0, 40.0));
        assert_eq!(p.port, pt(108.0, 40.0));
    }

    #[test]
    fn port_for_top_default() {
        // Node: anchor:{x:50,y:20}, port:{x:50,y:2}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let p = port_for(&r, "top", PORT_STUB, 0.0, false);
        assert_eq!(p.anchor, pt(50.0, 20.0));
        assert_eq!(p.port, pt(50.0, 2.0));
    }

    #[test]
    fn port_for_bottom_default() {
        // Node: anchor:{x:50,y:60}, port:{x:50,y:78}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let p = port_for(&r, "bottom", PORT_STUB, 0.0, false);
        assert_eq!(p.anchor, pt(50.0, 60.0));
        assert_eq!(p.port, pt(50.0, 78.0));
    }

    #[test]
    fn port_for_left_raw_offset_5() {
        // Node: portFor(rect,'left',18,5) → anchor:{x:10,y:45}, port:{x:-8,y:45}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let p = port_for(&r, "left", PORT_STUB, 5.0, false);
        assert_eq!(p.anchor, pt(10.0, 45.0));
        assert_eq!(p.port, pt(-8.0, 45.0));
    }

    #[test]
    fn port_for_fixed_ports_ignores_offset() {
        // Node: portFor({...fixedPorts:true},'left',18,5) → anchor:{x:10,y:40}, port:{x:-8,y:40}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let p = port_for(&r, "left", PORT_STUB, 5.0, true);
        assert_eq!(p.anchor, pt(10.0, 40.0));
        assert_eq!(p.port, pt(-8.0, 40.0));
    }

    // --- portCandidatesFor ---

    #[test]
    fn port_candidates_for_basic() {
        // Node: portCandidatesFor(rect,'left',[0,5,-5])
        // → offsets rounded/clamped = [0, 5, -5] (all within maxOffset=12)
        // anchor for 0 offset → {x:10,y:40}, port {x:-8,y:40}
        // anchor for 5 offset → {x:10,y:45}, port {x:-8,y:45}
        // anchor for -5 offset → {x:10,y:35}, port {x:-8,y:35}
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let candidates = port_candidates_for(&r, "left", &[0.0, 5.0, -5.0], false);
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].anchor, pt(10.0, 40.0));
        assert_eq!(candidates[1].anchor, pt(10.0, 45.0));
        assert_eq!(candidates[2].anchor, pt(10.0, 35.0));
    }

    #[test]
    fn port_candidates_for_deduplicates_after_round() {
        // Two offsets that round to the same value → deduplicated
        let r = rect(0.0, 0.0, 100.0, 100.0);
        let candidates = port_candidates_for(&r, "left", &[1.2, 1.4], false);
        // Both round to 1 → only one entry
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn port_candidates_for_fixed_ports_returns_one() {
        // Node: portCandidatesFor({...fixedPorts:true},'left',[0,5,-5]) → [{anchor:{x:10,y:40},port:{x:-8,y:40}}]
        let r = rect(10.0, 20.0, 80.0, 40.0);
        let candidates = port_candidates_for(&r, "left", &[0.0, 5.0, -5.0], true);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].anchor, pt(10.0, 40.0));
        assert_eq!(candidates[0].port, pt(-8.0, 40.0));
    }
}
