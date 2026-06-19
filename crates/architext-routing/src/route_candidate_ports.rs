//! Faithful port of `viewer/src/routing/routeCandidatePorts.js`.
//!
//! Translation decisions:
//! - `candidatePorts`: computes `sharedOffsets` using `targetAlignedStartOffset`
//!   and `targetAlignedEndOffset`, which pick the tangent-aligned component of
//!   the center-to-center vector. This matches JS exactly: `tangent.y !== 0`
//!   selects the Y component, otherwise X.
//! - `scope`: the `"grid"` scope produces 3 offsets `[0, from, to]`; the
//!   `"cheap"` scope (and any other value) produces 5 offsets
//!   `[0, from, to, from+targetAlignedStart, to+targetAlignedEnd]`. The
//!   deduplication inside `portCandidatesFor` (via `IndexSet` in `route_ports`)
//!   collapses duplicates after rounding and clamping, so the actual number of
//!   returned ports varies by geometry.
//! - `sidePairsFor`: the two `Math.abs` comparisons are `>=` (not `>`), so when
//!   horizontal and vertical displacements are exactly equal the horizontal pair
//!   wins for the primary slot and vertical for the secondary. This is preserved.
//! - `portPairsFor`: deduplication key is `"x1,y1:x2,y2"` on anchor coordinates.
//!   We use `IndexSet<String>` to match JS `Set` insertion-order semantics.
//! - No `js_hypot` / `js_round` calls here; all arithmetic is integer-like
//!   (center coordinates, simple subtraction). No hypot-in-comparison watch-items.

use indexmap::IndexSet;

use crate::model::{Point, Rect};
use crate::route_ports::{port_candidates_for, port_candidates_for_with_anchors, tangent_vector, PortResult, SideAnchors};

// ---------------------------------------------------------------------------
// candidatePorts
// ---------------------------------------------------------------------------

/// Scope controls how many offset candidates are generated for each port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateScope {
    /// `"cheap"` (default): 5 offsets — `[0, from, to, from+aligned, to+aligned]`.
    #[default]
    Cheap,
    /// `"grid"`: 3 offsets — `[0, from, to]`.
    Grid,
}

/// Endpoint offsets passed to `candidatePorts`. Mirrors the JS
/// `{ from: number, to: number }` object.
#[derive(Debug, Clone, Copy, Default)]
pub struct EndpointOffsets {
    pub from: f64,
    pub to: f64,
}

/// Return value of `candidatePorts`.
#[derive(Debug, Clone)]
pub struct CandidatePortSet {
    pub starts: Vec<PortResult>,
    pub ends: Vec<PortResult>,
}

/// Port of JS `candidatePorts(fromRect, toRect, startSide, endSide, endpointOffsets, scope)`.
///
/// Computes the set of candidate port positions for both endpoints given the
/// chosen side pair. The `scope` parameter controls how many offsets are tried:
/// `"grid"` uses 3 (no target-aligned extras); `"cheap"` uses 5 (adds the
/// target-aligned start and end offsets).
pub fn candidate_ports(
    from_rect: &Rect,
    to_rect: &Rect,
    start_side: &str,
    end_side: &str,
    endpoint_offsets: &EndpointOffsets,
    scope: CandidateScope,
) -> CandidatePortSet {
    let from_center = Point {
        x: from_rect.x + from_rect.width / 2.0,
        y: from_rect.y + from_rect.height / 2.0,
    };
    let to_center = Point {
        x: to_rect.x + to_rect.width / 2.0,
        y: to_rect.y + to_rect.height / 2.0,
    };
    let start_tangent = tangent_vector(start_side);
    let end_tangent = tangent_vector(end_side);

    // JS: tangentVector returns {x:0,y:1} for left/right sides, {x:1,y:0} for top/bottom.
    // `tangent.y !== 0` → left/right sides → use Y component of center-to-center vector.
    let target_aligned_start_offset = if start_tangent.y != 0.0 {
        to_center.y - from_center.y
    } else {
        to_center.x - from_center.x
    };
    let target_aligned_end_offset = if end_tangent.y != 0.0 {
        from_center.y - to_center.y
    } else {
        from_center.x - to_center.x
    };

    let shared_offsets: Vec<f64> = match scope {
        CandidateScope::Grid => vec![0.0, endpoint_offsets.from, endpoint_offsets.to],
        CandidateScope::Cheap => vec![
            0.0,
            endpoint_offsets.from,
            endpoint_offsets.to,
            endpoint_offsets.from + target_aligned_start_offset,
            endpoint_offsets.to + target_aligned_end_offset,
        ],
    };

    CandidatePortSet {
        starts: port_candidates_for(from_rect, start_side, &shared_offsets, false),
        ends: port_candidates_for(to_rect, end_side, &shared_offsets, false),
    }
}

/// `candidatePorts` with optional per-node `SideAnchors` overrides.
///
/// Decision-diamond nodes carry `sideAnchors` (diamond tips) instead of the
/// geometric rect-edge midpoint. JS stores these on the rect object itself;
/// we thread them here so `portCandidatesFor` returns the tip-anchored ports
/// rather than the geometric midpoint (matching JS `anchorFor(rect, side)`).
#[allow(clippy::too_many_arguments)]
pub fn candidate_ports_with_anchors(
    from_rect: &Rect,
    to_rect: &Rect,
    start_side: &str,
    end_side: &str,
    endpoint_offsets: &EndpointOffsets,
    scope: CandidateScope,
    from_side_anchors: Option<&SideAnchors>,
    to_side_anchors: Option<&SideAnchors>,
) -> CandidatePortSet {
    // Shared-offset computation is identical to candidate_ports.
    let from_center = Point {
        x: from_rect.x + from_rect.width / 2.0,
        y: from_rect.y + from_rect.height / 2.0,
    };
    let to_center = Point {
        x: to_rect.x + to_rect.width / 2.0,
        y: to_rect.y + to_rect.height / 2.0,
    };
    let start_tangent = tangent_vector(start_side);
    let end_tangent = tangent_vector(end_side);
    let target_aligned_start_offset = if start_tangent.y != 0.0 {
        to_center.y - from_center.y
    } else {
        to_center.x - from_center.x
    };
    let target_aligned_end_offset = if end_tangent.y != 0.0 {
        from_center.y - to_center.y
    } else {
        from_center.x - to_center.x
    };
    let shared_offsets: Vec<f64> = match scope {
        CandidateScope::Grid => vec![0.0, endpoint_offsets.from, endpoint_offsets.to],
        CandidateScope::Cheap => vec![
            0.0,
            endpoint_offsets.from,
            endpoint_offsets.to,
            endpoint_offsets.from + target_aligned_start_offset,
            endpoint_offsets.to + target_aligned_end_offset,
        ],
    };
    CandidatePortSet {
        starts: port_candidates_for_with_anchors(from_rect, start_side, &shared_offsets, false, from_side_anchors),
        ends: port_candidates_for_with_anchors(to_rect, end_side, &shared_offsets, false, to_side_anchors),
    }
}

// ---------------------------------------------------------------------------
// sidePairsFor
// ---------------------------------------------------------------------------

/// Port of JS `sidePairsFor(fromRect, toRect)`.
///
/// Returns an ordered, deduplicated list of `[startSide, endSide]` pairs
/// to try, starting with the geometrically dominant direction. Deduplication
/// is insertion-ordered via `IndexSet`.
///
/// The full 10-candidate list (before dedup) is:
/// 1. primary (dominant axis)
/// 2. secondary (other axis)
/// 3. `["left", "right"]`
/// 4. `["right", "left"]`
/// 5. `["top", "bottom"]`
/// 6. `["bottom", "top"]`
/// 7. `["left", "left"]`
/// 8. `["right", "right"]`
/// 9. `["top", "top"]`
/// 10. `["bottom", "bottom"]`
///
/// When horizontal displacement `>= ` vertical, horizontal pair is primary.
pub fn side_pairs_for(from_rect: &Rect, to_rect: &Rect) -> Vec<[&'static str; 2]> {
    let from_center = Point {
        x: from_rect.x + from_rect.width / 2.0,
        y: from_rect.y + from_rect.height / 2.0,
    };
    let to_center = Point {
        x: to_rect.x + to_rect.width / 2.0,
        y: to_rect.y + to_rect.height / 2.0,
    };

    // JS: toCenter.x >= fromCenter.x ? ["right","left"] : ["left","right"]
    let horizontal: [&'static str; 2] = if to_center.x >= from_center.x {
        ["right", "left"]
    } else {
        ["left", "right"]
    };
    // JS: toCenter.y >= fromCenter.y ? ["bottom","top"] : ["top","bottom"]
    let vertical: [&'static str; 2] = if to_center.y >= from_center.y {
        ["bottom", "top"]
    } else {
        ["top", "bottom"]
    };

    // JS: Math.abs(dx) >= Math.abs(dy) → horizontal is primary
    let dx = (to_center.x - from_center.x).abs();
    let dy = (to_center.y - from_center.y).abs();
    let (primary, secondary) = if dx >= dy {
        (horizontal, vertical)
    } else {
        (vertical, horizontal)
    };

    let candidates: [[&'static str; 2]; 10] = [
        primary,
        secondary,
        ["left", "right"],
        ["right", "left"],
        ["top", "bottom"],
        ["bottom", "top"],
        ["left", "left"],
        ["right", "right"],
        ["top", "top"],
        ["bottom", "bottom"],
    ];

    // JS: const seen = new Set(); return pairs.filter(([s,e]) => { key=`${s}:${e}`;
    //     if(seen.has(key)) return false; seen.add(key); return true; })
    let mut seen: IndexSet<String> = IndexSet::new();
    candidates
        .into_iter()
        .filter(|[s, e]| seen.insert(format!("{s}:{e}")))
        .collect()
}

// ---------------------------------------------------------------------------
// portPairsFor
// ---------------------------------------------------------------------------

/// Port of JS `portPairsFor(ports)`.
///
/// Returns all unique `[start, end]` port pairs from the cross product of
/// `ports.starts × ports.ends`. Uniqueness key is `"x1,y1:x2,y2"` on anchor
/// coordinates, matching JS `new Set(key)` insertion order.
pub fn port_pairs_for(ports: &CandidatePortSet) -> Vec<[PortResult; 2]> {
    let mut pairs = Vec::new();
    let mut seen: IndexSet<String> = IndexSet::new();
    for start in &ports.starts {
        for end in &ports.ends {
            let key = format!(
                "{},{}:{},{}",
                start.anchor.x, start.anchor.y, end.anchor.x, end.anchor.y
            );
            if seen.insert(key) {
                pairs.push([start.clone(), end.clone()]);
            }
        }
    }
    pairs
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

    fn pt(x: f64, y: f64) -> crate::model::Point {
        crate::model::Point { x, y }
    }

    // --- candidatePorts ---

    #[test]
    fn candidate_ports_cheap_zero_offsets_same_y() {
        // Node: fromRect={x:0,y:0,w:100,h:80}, toRect={x:200,y:0,w:100,h:80}
        // fromCenter={50,40}, toCenter={250,40}
        // startSide="right", endSide="left", offsets={from:0,to:0}, scope="cheap"
        // startTangent for "right" = {x:0,y:1} → tangent.y!=0 → targetAlignedStart = toY-fromY = 0
        // endTangent for "left" = {x:0,y:1} → targetAlignedEnd = fromY-toY = 0
        // sharedOffsets = [0,0,0,0,0] → deduped to [0] → 1 port each
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 0.0, to: 0.0 };
        let cp = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Cheap);
        // Node golden: starts count=1, start[0].anchor={x:100,y:40}
        assert_eq!(cp.starts.len(), 1);
        assert_eq!(cp.ends.len(), 1);
        assert_eq!(cp.starts[0].anchor, pt(100.0, 40.0));
        assert_eq!(cp.starts[0].port, pt(118.0, 40.0));
        assert_eq!(cp.ends[0].anchor, pt(200.0, 40.0));
    }

    #[test]
    fn candidate_ports_cheap_nonzero_offsets_same_y() {
        // Node: offsets={from:6,to:-6}, scope="cheap"
        // fromCenter={50,40}, toCenter={250,40} → targetAligned[start|end]=0
        // sharedOffsets = [0, 6, -6, 6+0=6(dup), -6+0=-6(dup)] → [0, 6, -6]
        // Node golden: starts count=3, anchors: {x:100,y:40},{x:100,y:46},{x:100,y:34}
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 6.0, to: -6.0 };
        let cp = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Cheap);
        assert_eq!(cp.starts.len(), 3);
        assert_eq!(cp.starts[0].anchor, pt(100.0, 40.0));
        assert_eq!(cp.starts[1].anchor, pt(100.0, 46.0));
        assert_eq!(cp.starts[2].anchor, pt(100.0, 34.0));
        assert_eq!(cp.ends.len(), 3);
        assert_eq!(cp.ends[0].anchor, pt(200.0, 40.0));
    }

    #[test]
    fn candidate_ports_grid_scope_three_offsets() {
        // Node: scope="grid" → sharedOffsets=[0, from, to] (no targetAligned)
        // offsets={from:0,to:0} → [0,0,0] → deduped to [0] → 1 port
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 0.0, to: 0.0 };
        let cp = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Grid);
        // Node golden: starts count=1
        assert_eq!(cp.starts.len(), 1);
    }

    #[test]
    fn candidate_ports_target_aligned_offsets_horizontal_side() {
        // fromRect={x:0,y:0,w:100,h:80}, toRect={x:150,y:0,w:100,h:80}
        // startSide="top", endSide="bottom", offsets={from:0,to:0}
        // fromCenter={50,40}, toCenter={200,40}
        // startTangent for "top" = {x:1,y:0} → tangent.y=0 → targetAlignedStart = toX-fromX = 150
        // endTangent for "bottom" = {x:1,y:0} → targetAlignedEnd = fromX-toX = -150
        // sharedOffsets = [0, 0, 0, 0+150=150, 0+(-150)=-150] → deduped: [0, 150, -150]
        // maxOffset for "top" = width/2 - 8 = 50-8 = 42 → clamp(150,-42,42)=42, clamp(-150,-42,42)=-42
        // After round: [0, 42, -42] → 3 unique anchors
        // Node golden: starts count=3, anchors: {x:50,y:0},{x:92,y:0},{x:8,y:0}
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(150.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 0.0, to: 0.0 };
        let cp = candidate_ports(&from, &to, "top", "bottom", &eo, CandidateScope::Cheap);
        assert_eq!(cp.starts.len(), 3);
        assert_eq!(cp.starts[0].anchor, pt(50.0, 0.0));
        assert_eq!(cp.starts[1].anchor, pt(92.0, 0.0));
        assert_eq!(cp.starts[2].anchor, pt(8.0, 0.0));
    }

    #[test]
    fn candidate_ports_cross_product_gives_nine_pairs() {
        // Node: fromRect={x:0,y:0,w:100,h:80}, toRect={x:200,y:0,w:100,h:80}
        // offsets={from:6,to:12}, scope="cheap"
        // starts and ends each have 3 unique ports → portPairsFor = 3×3 = 9
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 6.0, to: 12.0 };
        let cp = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Cheap);
        let pairs = port_pairs_for(&cp);
        // Node golden: portPairs count=9
        assert_eq!(pairs.len(), 9);
        // First pair: start[0].anchor={x:100,y:40}, end[0].anchor={x:200,y:40}
        assert_eq!(pairs[0][0].anchor, pt(100.0, 40.0));
        assert_eq!(pairs[0][1].anchor, pt(200.0, 40.0));
    }

    // --- sidePairsFor ---

    #[test]
    fn side_pairs_for_dominant_horizontal() {
        // fromCenter=(50,40), toCenter=(250,40): dx=200 >= dy=0 → horizontal primary
        // Node golden: [["right","left"],["bottom","top"],["left","right"],...]
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let pairs = side_pairs_for(&from, &to);
        assert_eq!(pairs[0], ["right", "left"]);
        assert_eq!(pairs[1], ["bottom", "top"]); // secondary vertical (toY>=fromY → bottom/top)
        // Fixed pairs follow; check the full deduped list has correct count
        // After dedup of 10 candidates: primary + secondary already cover right:left and bottom:top
        // Remaining fixed: left:right, right:left(dup), top:bottom, bottom:top(dup),
        //   left:left, right:right, top:top, bottom:bottom → 6 new
        assert_eq!(pairs.len(), 8); // 2 geometric + 6 fixed non-duplicate
        assert_eq!(pairs[2], ["left", "right"]);
        assert_eq!(pairs[3], ["top", "bottom"]);
        assert_eq!(pairs[4], ["left", "left"]);
        assert_eq!(pairs[5], ["right", "right"]);
        assert_eq!(pairs[6], ["top", "top"]);
        assert_eq!(pairs[7], ["bottom", "bottom"]);
    }

    #[test]
    fn side_pairs_for_dominant_vertical() {
        // fromCenter=(50,40), toCenter=(50,240): dx=0 < dy=200 → vertical primary
        // Node golden: [["bottom","top"],["right","left"],["left","right"],...]
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(0.0, 200.0, 100.0, 80.0);
        let pairs = side_pairs_for(&from, &to);
        assert_eq!(pairs[0], ["bottom", "top"]);
        assert_eq!(pairs[1], ["right", "left"]); // secondary horizontal (toX>=fromX → right/left)
        assert_eq!(pairs.len(), 8);
    }

    #[test]
    fn side_pairs_for_deduplicates() {
        // When primary/secondary already match a fixed pair, the fixed pair is dropped.
        // ["right","left"] at index 0 → fixed pair 3 ("right","left") is a dup → filtered
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let pairs = side_pairs_for(&from, &to);
        // Verify no duplicate keys
        let keys: Vec<String> = pairs.iter().map(|[s, e]| format!("{s}:{e}")).collect();
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(keys.len(), unique.len(), "side_pairs_for returned duplicates");
    }

    // --- portPairsFor ---

    #[test]
    fn port_pairs_for_single_start_single_end() {
        // Node: candidatePorts(fromRect,toRect,"right","left",{from:0,to:0},"cheap")
        //   → 1 start, 1 end → 1 pair
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 0.0, to: 0.0 };
        let cp = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Cheap);
        let pairs = port_pairs_for(&cp);
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn port_pairs_for_deduplicates_same_anchor() {
        // Node: if start has duplicate anchor coords, portPairsFor deduplicates
        // Build a CandidatePortSet with two starts sharing the same anchor
        let shared_port = PortResult {
            anchor: pt(10.0, 40.0),
            port: pt(-8.0, 40.0),
        };
        let ports = CandidatePortSet {
            starts: vec![shared_port.clone(), shared_port.clone()],
            ends: vec![PortResult { anchor: pt(90.0, 40.0), port: pt(108.0, 40.0) }],
        };
        let pairs = port_pairs_for(&ports);
        // Node golden: portPairsFor dedup length = 1 (two starts with same anchor → 1 pair)
        assert_eq!(pairs.len(), 1);
    }

    // --- candidate_ports_with_anchors (TDD: side-anchors override for decision-diamond nodes) ---

    #[test]
    fn candidate_ports_with_anchors_uses_side_anchor_for_from() {
        // Decision-diamond node: fromRect={x:0,y:0,w:100,h:80} has sideAnchors.right={x:100,y:20}
        // (the diamond tip instead of the geometric midpoint {x:100,y:40}).
        // Without sideAnchors: starts[0].anchor.y = 40.0 (geometric midpoint).
        // With sideAnchors: starts[0].anchor = {x:100, y:20} (diamond tip).
        // Node source: JS anchorFor(rect, side) returns rect.sideAnchors[side] when present.
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 0.0, to: 0.0 };

        // Without sideAnchors: geometric midpoint
        let cp_plain = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Cheap);
        assert_eq!(cp_plain.starts[0].anchor, pt(100.0, 40.0), "geometric midpoint");

        // With sideAnchors on fromRect: anchor overridden to diamond tip
        let from_anchors = SideAnchors {
            right: Some(pt(100.0, 20.0)), // diamond right-tip is above midpoint
            ..Default::default()
        };
        let cp_anchored = candidate_ports_with_anchors(
            &from, &to, "right", "left", &eo, CandidateScope::Cheap,
            Some(&from_anchors), None,
        );
        assert_eq!(
            cp_anchored.starts[0].anchor,
            pt(100.0, 20.0),
            "side_anchors override: diamond tip, not geometric midpoint"
        );
        // Absent side (left) still uses geometric midpoint
        assert_eq!(cp_anchored.starts[0].port, pt(118.0, 20.0), "port offset from tip anchor");
        // toRect has no sideAnchors → geometric midpoint unchanged
        assert_eq!(cp_anchored.ends[0].anchor, pt(200.0, 40.0), "to geometric midpoint unchanged");
    }

    #[test]
    fn candidate_ports_with_anchors_no_override_matches_candidate_ports() {
        // When both from_side_anchors and to_side_anchors are None, output must be
        // identical to candidate_ports (no regression on non-diamond nodes).
        let from = rect(0.0, 0.0, 100.0, 80.0);
        let to = rect(200.0, 0.0, 100.0, 80.0);
        let eo = EndpointOffsets { from: 6.0, to: -6.0 };
        let cp = candidate_ports(&from, &to, "right", "left", &eo, CandidateScope::Cheap);
        let cp_with = candidate_ports_with_anchors(
            &from, &to, "right", "left", &eo, CandidateScope::Cheap, None, None,
        );
        assert_eq!(cp.starts.len(), cp_with.starts.len());
        assert_eq!(cp.ends.len(), cp_with.ends.len());
        for (a, b) in cp.starts.iter().zip(cp_with.starts.iter()) {
            assert_eq!(a.anchor, b.anchor);
            assert_eq!(a.port, b.port);
        }
    }
}
