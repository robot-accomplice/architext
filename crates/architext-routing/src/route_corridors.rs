//! Faithful port of `viewer/src/routing/routeCorridors.js`.
//!
//! Depends on `route_ports` (for `PORT_STUB`), `route_constants` (for
//! `CANVAS_INSET`, `dedupe_by`), and `route_geometry` (for `unique_rounded`).
//!
//! Translation decisions:
//! - `Math.round` → `js_compat::js_round` (used in `gutterLaneValues` and
//!   `interiorCorridors`).
//! - `Array.from({length:n}, (_,i) => ...)` → `(0..n).map(|i| ...)`.
//! - `.filter(v => v > min && v < max)` — strict inequality, exact.
//! - `verticalEdges.sort((a,b) => a-b)` is a numeric ascending sort (no ties
//!   change order for distinct rounded integers). We use `f64::total_cmp`
//!   which agrees with JS numeric sort for finite values.
//! - `uniqueRounded` imported from `crate::route_geometry`.
//! - `dedupeBy` imported from `crate::route_constants`.
//! - `PORT_STUB` imported from `crate::route_ports`.
//! - `CANVAS_INSET` imported from `crate::route_constants`.
//! - `.at(-1)` on a sorted vec → `.last()`.
//! - `.find(c => c.value > max)` on sorted array → iterator `.find()`.
//! - The JS `mergeCorridors` (dedupeBy on `axis:value` key) is reproduced via
//!   `dedupe_by` with a string key `"x:N"` or `"y:N"` using
//!   `js_number_to_string` so the key exactly matches JS template literals.

use crate::js_compat::{js_number_to_string, js_round};
use crate::model::Rect;
use crate::route_constants::{dedupe_by, CANVAS_INSET};
use crate::route_geometry::unique_rounded;
use crate::route_ports::PORT_STUB;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum corridor width before gutter lanes are added.
pub const CORRIDOR_PADDING: f64 = 10.0;

const GUTTER_LANE_SPACING: f64 = 42.0;
const MAX_GUTTER_LANES: usize = 6;

// ---------------------------------------------------------------------------
// Corridor type
// ---------------------------------------------------------------------------

/// A routing corridor: an axis-aligned guide line at `value` on `axis`.
/// `axis == "x"` means a vertical lane at x=value; `axis == "y"` a horizontal
/// lane at y=value.
#[derive(Debug, Clone, PartialEq)]
pub struct Corridor {
    /// `"x"` or `"y"`.
    pub axis: String,
    pub value: f64,
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Port of JS `gutterLaneValues(start, end, min, max)`.
///
/// Returns up to `MAX_GUTTER_LANES` evenly-spaced lane positions inside the
/// `(start, end)` span, filtered to `(min, max)` exclusive.
fn gutter_lane_values(start: f64, end: f64, min: f64, max: f64) -> Vec<f64> {
    let width = end - start;
    if width <= CORRIDOR_PADDING * 3.0 {
        return vec![];
    }
    let lane_count = usize::min(
        MAX_GUTTER_LANES,
        usize::max(1, f64::floor(width / GUTTER_LANE_SPACING) as usize),
    );
    (0..lane_count)
        .map(|index| {
            js_round(start + (width * (index + 1) as f64) / (lane_count + 1) as f64)
        })
        .filter(|&value| value > min && value < max)
        .collect()
}

/// Port of JS `interiorCorridors(fromRect, toRect)`.
fn interior_corridors(from_rect: &Rect, to_rect: &Rect) -> Vec<Corridor> {
    let mut corridors = Vec::new();

    // Vertical gap midpoint
    let vertical_gap_start =
        f64::min(from_rect.y, to_rect.y) + f64::min(from_rect.height, to_rect.height);
    let vertical_gap_end = f64::max(from_rect.y, to_rect.y);
    if vertical_gap_end - vertical_gap_start > PORT_STUB * 2.0 {
        corridors.push(Corridor {
            axis: "y".to_string(),
            value: js_round((vertical_gap_start + vertical_gap_end) / 2.0),
        });
    }

    // Horizontal gap midpoint
    let horizontal_gap_start =
        f64::min(from_rect.x, to_rect.x) + f64::min(from_rect.width, to_rect.width);
    let horizontal_gap_end = f64::max(from_rect.x, to_rect.x);
    if horizontal_gap_end - horizontal_gap_start > PORT_STUB * 2.0 {
        corridors.push(Corridor {
            axis: "x".to_string(),
            value: js_round((horizontal_gap_start + horizontal_gap_end) / 2.0),
        });
    }

    corridors
}

/// Port of JS `mergeCorridors(corridors)`.
///
/// Deduplicates by `"axis:value"` key (first-occurrence wins), where `value`
/// is formatted with `js_number_to_string` to match JS template literal output.
fn merge_corridors(corridors: Vec<Corridor>) -> Vec<Corridor> {
    dedupe_by(corridors, |c| {
        format!("{}:{}", c.axis, js_number_to_string(c.value))
    })
}

// ---------------------------------------------------------------------------
// Public exports
// ---------------------------------------------------------------------------

/// Port of JS `freeSpaceCorridors(visibleRects, canvasWidth, canvasHeight)`.
///
/// Computes corridor guide lines from the free space between nodes and the
/// canvas edges.
pub fn free_space_corridors(
    visible_rects: &[Rect],
    canvas_width: f64,
    canvas_height: f64,
) -> Vec<Corridor> {
    let min_x = CANVAS_INSET.left;
    let max_x = canvas_width - CANVAS_INSET.right;
    let min_y = CANVAS_INSET.top;
    let max_y = canvas_height - CANVAS_INSET.bottom;

    // Collect all x-edges (vertical boundaries): canvas inset + rect left/right
    let mut x_edge_vals: Vec<f64> = vec![min_x, max_x];
    for rect in visible_rects {
        x_edge_vals.push(rect.x);
        x_edge_vals.push(rect.x + rect.width);
    }
    let mut vertical_edges = unique_rounded(&x_edge_vals);
    vertical_edges.sort_by(|a, b| a.total_cmp(b));

    // Collect all y-edges (horizontal boundaries): canvas inset + rect top/bottom
    let mut y_edge_vals: Vec<f64> = vec![min_y, max_y];
    for rect in visible_rects {
        y_edge_vals.push(rect.y);
        y_edge_vals.push(rect.y + rect.height);
    }
    let mut horizontal_edges = unique_rounded(&y_edge_vals);
    horizontal_edges.sort_by(|a, b| a.total_cmp(b));

    let mut corridors: Vec<Corridor> = Vec::new();

    // x-axis corridors (vertical lanes)
    for index in 0..vertical_edges.len().saturating_sub(1) {
        let left = vertical_edges[index];
        let right = vertical_edges[index + 1];
        for value in gutter_lane_values(left, right, min_x, max_x) {
            corridors.push(Corridor { axis: "x".to_string(), value });
        }
    }

    // y-axis corridors (horizontal lanes)
    for index in 0..horizontal_edges.len().saturating_sub(1) {
        let top = horizontal_edges[index];
        let bottom = horizontal_edges[index + 1];
        for value in gutter_lane_values(top, bottom, min_y, max_y) {
            corridors.push(Corridor { axis: "y".to_string(), value });
        }
    }

    corridors
}

/// Options for `edgeCorridors`.
#[derive(Debug, Clone, Default)]
pub struct EdgeCorridorOptions {
    /// When true, include the nearest x/y corridors outside the edge bounding
    /// box (for horizontal/vertical overlap cases respectively).
    pub include_exterior: bool,
}

/// Port of JS `edgeCorridors(fromRect, toRect, diagramCorridors, options)`.
///
/// Returns the subset of `diagram_corridors` relevant to an edge between
/// `from_rect` and `to_rect`, plus interior gap midpoints.
pub fn edge_corridors(
    from_rect: &Rect,
    to_rect: &Rect,
    diagram_corridors: &[Corridor],
    options: &EdgeCorridorOptions,
) -> Vec<Corridor> {
    let min_x = f64::min(from_rect.x, to_rect.x) - PORT_STUB * 2.0;
    let max_x = f64::max(from_rect.x + from_rect.width, to_rect.x + to_rect.width) + PORT_STUB * 2.0;
    let min_y = f64::min(from_rect.y, to_rect.y) - PORT_STUB * 2.0;
    let max_y = f64::max(from_rect.y + from_rect.height, to_rect.y + to_rect.height) + PORT_STUB * 2.0;

    let midpoint_x = (from_rect.x + from_rect.width / 2.0 + to_rect.x + to_rect.width / 2.0) / 2.0;
    let midpoint_y = (from_rect.y + from_rect.height / 2.0 + to_rect.y + to_rect.height / 2.0) / 2.0;

    // `localCorridors`: diagram corridors within the bounding box
    let local_corridors: Vec<&Corridor> = diagram_corridors
        .iter()
        .filter(|c| {
            if c.axis == "x" {
                c.value >= min_x && c.value <= max_x
            } else {
                c.value >= min_y && c.value <= max_y
            }
        })
        .collect();

    // `closest(axis)`: up to 6 corridors on `axis`, sorted by distance to midpoint
    let closest = |axis: &str| -> Vec<Corridor> {
        let midpoint = if axis == "x" { midpoint_x } else { midpoint_y };
        let mut filtered: Vec<&Corridor> = local_corridors
            .iter()
            .filter(|c| c.axis == axis)
            .copied()
            .collect();
        filtered.sort_by(|a, b| {
            f64::abs(a.value - midpoint).total_cmp(&f64::abs(b.value - midpoint))
        });
        filtered.into_iter().take(6).cloned().collect()
    };

    // `exterior(axis, min, max)`: nearest corridor before `min` and after `max`
    let exterior = |axis: &str, ext_min: f64, ext_max: f64| -> Vec<Corridor> {
        let mut axis_corridors: Vec<&Corridor> = diagram_corridors
            .iter()
            .filter(|c| c.axis == axis)
            .collect();
        axis_corridors.sort_by(|a, b| a.value.total_cmp(&b.value));

        // JS `.filter(c => c.value < min).at(-1)` = last element before min.
        // rfind on iter() returns Option<&&Corridor>; double-deref to get Corridor clone.
        let before: Option<Corridor> = axis_corridors
            .iter()
            .rfind(|c| c.value < ext_min)
            .map(|c| (*c).clone());
        let after: Option<Corridor> = axis_corridors
            .iter()
            .find(|c| c.value > ext_max)
            .map(|c| (*c).clone());

        [before, after].into_iter().flatten().collect()
    };

    // JS: horizontalOverlap = fromRect.x < toRect.x+toRect.width && fromRect.x+fromRect.width > toRect.x
    let horizontal_overlap = from_rect.x < to_rect.x + to_rect.width
        && from_rect.x + from_rect.width > to_rect.x;
    let vertical_overlap = from_rect.y < to_rect.y + to_rect.height
        && from_rect.y + from_rect.height > to_rect.y;

    let exterior_corridors: Vec<Corridor> = if options.include_exterior {
        let mut ext = Vec::new();
        if horizontal_overlap {
            ext.extend(exterior("x", min_x, max_x));
        }
        if vertical_overlap {
            ext.extend(exterior("y", min_y, max_y));
        }
        ext
    } else {
        vec![]
    };

    merge_corridors(
        interior_corridors(from_rect, to_rect)
            .into_iter()
            .chain(exterior_corridors)
            .chain(closest("x"))
            .chain(closest("y"))
            .collect(),
    )
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

    // --- CORRIDOR_PADDING constant ---

    #[test]
    fn corridor_padding_constant() {
        // Node: CORRIDOR_PADDING = 10
        assert_eq!(CORRIDOR_PADDING, 10.0);
    }

    // --- freeSpaceCorridors ---

    #[test]
    fn free_space_corridors_two_rects() {
        // Node: freeSpaceCorridors([{x:100,y:100,w:80,h:40},{x:300,y:200,w:80,h:40}], 500, 400)
        // → 15 corridors (7 x-axis, 8 y-axis… verified by golden list below)
        let rects = vec![
            rect(100.0, 100.0, 80.0, 40.0),
            rect(300.0, 200.0, 80.0, 40.0),
        ];
        let corridors = free_space_corridors(&rects, 500.0, 400.0);
        // Node golden (exact order):
        // x:62, x:140, x:220, x:260, x:340, x:412, x:444
        // y:65, y:120, y:170, y:220, y:274, y:308, y:342
        let expected: Vec<(&str, f64)> = vec![
            ("x", 62.0), ("x", 140.0), ("x", 220.0), ("x", 260.0), ("x", 340.0), ("x", 412.0), ("x", 444.0),
            ("y", 65.0), ("y", 120.0), ("y", 170.0), ("y", 220.0), ("y", 274.0), ("y", 308.0), ("y", 342.0),
        ];
        assert_eq!(corridors.len(), expected.len(), "corridor count mismatch");
        for (i, (axis, value)) in expected.iter().enumerate() {
            assert_eq!(&corridors[i].axis, *axis, "axis mismatch at {i}");
            assert_eq!(corridors[i].value, *value, "value mismatch at {i}");
        }
    }

    #[test]
    fn free_space_corridors_one_rect() {
        // Node: freeSpaceCorridors([{x:100,y:100,w:80,h:40}], 500, 400)
        // x: 62, 140, 222, 265, 307, 349, 391, 434
        // y: 65, 120, 179, 219, 258, 297, 337
        let rects = vec![rect(100.0, 100.0, 80.0, 40.0)];
        let corridors = free_space_corridors(&rects, 500.0, 400.0);
        let expected: Vec<(&str, f64)> = vec![
            ("x", 62.0), ("x", 140.0), ("x", 222.0), ("x", 265.0), ("x", 307.0), ("x", 349.0), ("x", 391.0), ("x", 434.0),
            ("y", 65.0), ("y", 120.0), ("y", 179.0), ("y", 219.0), ("y", 258.0), ("y", 297.0), ("y", 337.0),
        ];
        assert_eq!(corridors.len(), expected.len());
        for (i, (axis, value)) in expected.iter().enumerate() {
            assert_eq!(&corridors[i].axis, *axis, "axis[{i}]");
            assert_eq!(corridors[i].value, *value, "value[{i}]");
        }
    }

    // --- edgeCorridors ---

    #[test]
    fn edge_corridors_basic() {
        // Node: edgeCorridors(r1,r2, diagramCorridors) where r1={x:0,y:0,w:80,h:40}, r2={x:200,y:0,...}
        // diagram built from freeSpaceCorridors([r1,r2], 500, 400)
        // Node golden: [{axis:"x",value:140},{axis:"x",value:120},{axis:"x",value:160},{axis:"x",value:52},{axis:"x",value:240}]
        let r1 = rect(0.0, 0.0, 80.0, 40.0);
        let r2 = rect(200.0, 0.0, 80.0, 40.0);
        let diagram = free_space_corridors(&[r1.clone(), r2.clone()], 500.0, 400.0);
        let opts = EdgeCorridorOptions::default();
        let corridors = edge_corridors(&r1, &r2, &diagram, &opts);
        let expected: Vec<(&str, f64)> = vec![
            ("x", 140.0),
            ("x", 120.0),
            ("x", 160.0),
            ("x", 52.0),
            ("x", 240.0),
        ];
        assert_eq!(corridors.len(), expected.len(), "got {:?}", corridors);
        for (i, (axis, value)) in expected.iter().enumerate() {
            assert_eq!(&corridors[i].axis, *axis, "axis[{i}]");
            assert_eq!(corridors[i].value, *value, "value[{i}]");
        }
    }

    #[test]
    fn edge_corridors_with_exterior() {
        // Node: edgeCorridors(r1,r2, diagramCorridors, {includeExterior:true})
        // → [{x:140},{y:88},{x:120},{x:160},{x:52},{x:240}]
        let r1 = rect(0.0, 0.0, 80.0, 40.0);
        let r2 = rect(200.0, 0.0, 80.0, 40.0);
        let diagram = free_space_corridors(&[r1.clone(), r2.clone()], 500.0, 400.0);
        let opts = EdgeCorridorOptions { include_exterior: true };
        let corridors = edge_corridors(&r1, &r2, &diagram, &opts);
        let expected: Vec<(&str, f64)> = vec![
            ("x", 140.0),
            ("y", 88.0),
            ("x", 120.0),
            ("x", 160.0),
            ("x", 52.0),
            ("x", 240.0),
        ];
        assert_eq!(corridors.len(), expected.len(), "got {:?}", corridors);
        for (i, (axis, value)) in expected.iter().enumerate() {
            assert_eq!(&corridors[i].axis, *axis, "axis[{i}]");
            assert_eq!(corridors[i].value, *value, "value[{i}]");
        }
    }

    #[test]
    fn edge_corridors_vertically_spaced_interior() {
        // Node: edgeCorridors(r3={x:0,y:0,w:80,h:40}, r4={x:0,y:200,...}, [])
        // → interiorCorridors finds vertical gap: gapStart=40, gapEnd=200 → midpoint=120
        // → [{axis:"y",value:120}]
        let r3 = rect(0.0, 0.0, 80.0, 40.0);
        let r4 = rect(0.0, 200.0, 80.0, 40.0);
        let corridors = edge_corridors(&r3, &r4, &[], &EdgeCorridorOptions::default());
        assert_eq!(corridors.len(), 1);
        assert_eq!(corridors[0].axis, "y");
        assert_eq!(corridors[0].value, 120.0);
    }

    #[test]
    fn merge_corridors_deduplicates() {
        // Same axis:value key → first wins
        let dupes = vec![
            Corridor { axis: "x".to_string(), value: 100.0 },
            Corridor { axis: "x".to_string(), value: 100.0 },
            Corridor { axis: "y".to_string(), value: 50.0 },
        ];
        let merged = merge_corridors(dupes);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].value, 100.0);
        assert_eq!(merged[1].value, 50.0);
    }
}
