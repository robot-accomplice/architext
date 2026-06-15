//! Faithful port of `viewer/src/routing/routeIntent.js`.
//!
//! Translation decisions:
//! - All internal helpers (`rectCenter`, `laneDirection`, `rowDirection`,
//!   `corridorBlockerCount`, `semanticPrimaryCorridorBlocked`,
//!   `primarySurfaceBandBlocked`, `escapeSideFor`) are private to this module,
//!   matching JS module-private functions.
//! - `segmentIntersectsRect` imported from `crate::route_geometry`.
//! - `rectCenter` is already in `crate::route_constants` but the JS module
//!   defines its own local copy; we reuse the existing one (`rect_center` from
//!   `route_constants`) since it is identical.
//! - The JS `semanticSurfaceOptions` returns `{ source: Set, target: Set }`.
//!   We use `IndexSet<String>` for both to preserve JS Set insertion-order
//!   semantics (important for downstream iteration determinism).
//! - `relationship?.kind === "return" || Boolean(relationship?.returnOf)` →
//!   `is_return_kind` helper that checks both fields.
//! - Falsy string check (`!canvasHeight`, `!canvasWidth`) → `== 0.0` (canvas
//!   dimensions are always non-negative; 0 is the only falsy numeric value for
//!   finite, non-NaN inputs).
//! - `relationship.returnOf` is `Option<String>`; presence (Some(_)) is the
//!   truthy check (Boolean(relationship?.returnOf) in JS).

use indexmap::IndexSet;

use crate::model::Rect;
use crate::route_constants::rect_center;
use crate::route_geometry::segment_intersects_rect;

// ---------------------------------------------------------------------------
// Public output types
// ---------------------------------------------------------------------------

/// Output of `deriveRouteIntent`. All string fields use `&'static str` or
/// owned `String` to avoid lifetime complexity at call sites.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteIntent {
    pub relationship_id: String,
    pub role: String,
    /// JS `returnOf` — present only when `relationship.returnOf` is set.
    pub return_of: Option<String>,
    /// JS `outcome` — present only when `relationship.outcome` is set.
    pub outcome: Option<String>,
    pub lane_direction: String,
    pub row_direction: String,
    pub expected_source_side: String,
    pub expected_target_side: String,
}

/// A pair of source/target side strings (e.g., `"right"` / `"left"`).
#[derive(Debug, Clone, PartialEq)]
pub struct SidePair {
    pub source: String,
    pub target: String,
}

/// Return value of `semanticSurfaceOptions`.
#[derive(Debug, Clone)]
pub struct SurfaceOptions {
    /// JS Set of allowed source sides (insertion-ordered).
    pub source: IndexSet<String>,
    /// JS Set of allowed target sides (insertion-ordered).
    pub target: IndexSet<String>,
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Subset of the relationship object used across this module.
#[derive(Debug, Clone, Default)]
pub struct IntentRelationship {
    pub id: String,
    /// JS `relationship.kind`
    pub kind: Option<String>,
    /// JS `relationship.returnOf`
    pub return_of: Option<String>,
    /// JS `relationship.outcome`
    pub outcome: Option<String>,
    /// JS `relationship.relationshipType`
    pub relationship_type: Option<String>,
    /// JS `relationship.stepId`
    pub step_id: Option<String>,
    /// JS `relationship.flowId`
    pub flow_id: Option<String>,
    /// JS `relationship.preferredStartSide`
    pub preferred_start_side: Option<String>,
    /// JS `relationship.preferredEndSide`
    pub preferred_end_side: Option<String>,
}

/// Input to `deriveRouteIntent`.
pub struct DeriveRouteIntentInput<'a> {
    pub relationship: &'a IntentRelationship,
    pub from_rect: &'a Rect,
    pub to_rect: &'a Rect,
    pub from_lane_index: i64,
    pub to_lane_index: i64,
    pub from_row_index: i64,
    pub to_row_index: i64,
}

/// Input to `semanticSurfaceOptions`.
pub struct SemanticSurfaceOptionsInput<'a> {
    pub expected_sides: SidePair,
    pub relationship: &'a IntentRelationship,
    pub from_rect: &'a Rect,
    pub to_rect: &'a Rect,
    pub blocker_rects: Vec<Rect>,
    pub canvas_width: f64,
    pub canvas_height: f64,
}

// ---------------------------------------------------------------------------
// Private helpers (module-internal, matching JS non-exported functions)
// ---------------------------------------------------------------------------

fn lane_direction(from_lane: i64, to_lane: i64) -> &'static str {
    if from_lane == to_lane { "same" }
    else if from_lane < to_lane { "forward" }
    else { "backward" }
}

fn row_direction(from_row: i64, to_row: i64) -> &'static str {
    if from_row == to_row { "same" }
    else if from_row < to_row { "down" }
    else { "up" }
}

fn relationship_role(relationship: &IntentRelationship) -> String {
    if let Some(kind) = &relationship.kind {
        return kind.clone();
    }
    if relationship.return_of.is_some() {
        return "return".to_string();
    }
    if relationship.outcome.is_some() {
        return "decision-outcome".to_string();
    }
    "process".to_string()
}

/// Port of JS `corridorBlockerCount(fromRect, toRect, blockerRects)`.
fn corridor_blocker_count(from_rect: &Rect, to_rect: &Rect, blocker_rects: &[Rect]) -> usize {
    let padding = 12.0;
    let lo_y = f64::min(from_rect.y + from_rect.height, to_rect.y + to_rect.height);
    let hi_y = f64::max(from_rect.y, to_rect.y);
    let left = f64::max(from_rect.x, to_rect.x) - padding;
    let right = f64::min(from_rect.x + from_rect.width, to_rect.x + to_rect.width) + padding;
    if hi_y <= lo_y || right <= left {
        return 0;
    }
    blocker_rects
        .iter()
        .filter(|blocker| {
            blocker.y >= lo_y
                && blocker.y + blocker.height <= hi_y
                && blocker.x < right
                && blocker.x + blocker.width > left
        })
        .count()
}

/// Port of JS `primarySurfaceBandBlocked(fromRect, toRect, blocker, horizontalIntent)`.
fn primary_surface_band_blocked(
    from_rect: &Rect,
    to_rect: &Rect,
    blocker: &Rect,
    horizontal_intent: bool,
) -> bool {
    let padding = 12.0;
    if horizontal_intent {
        let left = f64::min(from_rect.x + from_rect.width, to_rect.x + to_rect.width);
        let right = f64::max(from_rect.x, to_rect.x);
        let top = f64::max(from_rect.y, to_rect.y) - padding;
        let bottom = f64::min(from_rect.y + from_rect.height, to_rect.y + to_rect.height) + padding;
        if right <= left || bottom <= top {
            return false;
        }
        return blocker.x < right
            && blocker.x + blocker.width > left
            && blocker.y < bottom
            && blocker.y + blocker.height > top;
    }
    // vertical intent
    let top = f64::min(from_rect.y + from_rect.height, to_rect.y + to_rect.height);
    let bottom = f64::max(from_rect.y, to_rect.y);
    let left = f64::max(from_rect.x, to_rect.x) - padding;
    let right = f64::min(from_rect.x + from_rect.width, to_rect.x + to_rect.width) + padding;
    if bottom <= top || right <= left {
        return false;
    }
    blocker.y < bottom
        && blocker.y + blocker.height > top
        && blocker.x < right
        && blocker.x + blocker.width > left
}

/// Port of JS `semanticPrimaryCorridorBlocked(context, expectedSides)`.
fn semantic_primary_corridor_blocked(
    relationship: &IntentRelationship,
    from_rect: &Rect,
    to_rect: &Rect,
    blocker_rects: &[Rect],
    expected_sides: &SidePair,
) -> bool {
    // JS: if no semantic fields present, return false
    if relationship.relationship_type.is_none()
        && relationship.kind.is_none()
        && relationship.return_of.is_none()
        && relationship.outcome.is_none()
        && relationship.step_id.is_none()
        && relationship.flow_id.is_none()
    {
        return false;
    }

    let horizontal_intent = (expected_sides.source == "right" && expected_sides.target == "left")
        || (expected_sides.source == "left" && expected_sides.target == "right");
    let vertical_intent = (expected_sides.source == "bottom" && expected_sides.target == "top")
        || (expected_sides.source == "top" && expected_sides.target == "bottom");

    if !horizontal_intent && !vertical_intent {
        return false;
    }

    let source_center = rect_center(from_rect);
    let target_center = rect_center(to_rect);

    blocker_rects.iter().any(|rect| {
        primary_surface_band_blocked(from_rect, to_rect, rect, horizontal_intent)
            || segment_intersects_rect(&source_center, &target_center, rect, 12.0)
    })
}

/// Port of JS `escapeSideFor(rect, expectedSide, canvasWidth, canvasHeight)`.
///
/// Returns an empty string if `rect` is absent. In Rust we always have `rect`,
/// so we never return `""` — callers gate on Some/None at the `source.add` call.
/// The `canvasWidth`/`canvasHeight == 0.0` check reproduces JS falsy-zero.
fn escape_side_for(rect: &Rect, expected_side: &str, canvas_width: f64, canvas_height: f64) -> String {
    let center = rect_center(rect);
    if expected_side == "left" || expected_side == "right" {
        // Horizontal intent: escape vertically
        if canvas_height == 0.0 {
            return if center.y < rect.height { "bottom".to_string() } else { "top".to_string() };
        }
        return if center.y < canvas_height / 2.0 { "bottom".to_string() } else { "top".to_string() };
    }
    // Vertical intent: escape sideways toward nearer gutter
    if canvas_width == 0.0 {
        return if center.x < rect.width { "left".to_string() } else { "right".to_string() };
    }
    if center.x < canvas_width / 2.0 { "left".to_string() } else { "right".to_string() }
}

// ---------------------------------------------------------------------------
// Public exports
// ---------------------------------------------------------------------------

/// Port of JS `expectedFacingSides(fromRect, toRect)`.
///
/// Determines the natural facing sides based on center-to-center direction.
/// Horizontal dominates when `|dx| >= |dy|` (JS `>=` means ties go horizontal).
pub fn expected_facing_sides(from_rect: &Rect, to_rect: &Rect) -> SidePair {
    let from = rect_center(from_rect);
    let to = rect_center(to_rect);
    if f64::abs(to.x - from.x) >= f64::abs(to.y - from.y) {
        return if to.x >= from.x {
            SidePair { source: "right".to_string(), target: "left".to_string() }
        } else {
            SidePair { source: "left".to_string(), target: "right".to_string() }
        };
    }
    if to.y >= from.y {
        SidePair { source: "bottom".to_string(), target: "top".to_string() }
    } else {
        SidePair { source: "top".to_string(), target: "bottom".to_string() }
    }
}

/// Port of JS `expectedRouteSides(fromRect, toRect, laneDirection, rowDirection)`.
pub fn expected_route_sides(
    from_rect: &Rect,
    to_rect: &Rect,
    lane_dir: &str,
    row_dir: &str,
) -> SidePair {
    match lane_dir {
        "forward" => SidePair { source: "right".to_string(), target: "left".to_string() },
        "backward" => SidePair { source: "left".to_string(), target: "right".to_string() },
        _ => match row_dir {
            "down" => SidePair { source: "bottom".to_string(), target: "top".to_string() },
            "up" => SidePair { source: "top".to_string(), target: "bottom".to_string() },
            _ => expected_facing_sides(from_rect, to_rect),
        },
    }
}

/// Port of JS `deriveRouteIntent(input)`.
pub fn derive_route_intent(input: &DeriveRouteIntentInput<'_>) -> RouteIntent {
    let lane = lane_direction(input.from_lane_index, input.to_lane_index);
    let row = row_direction(input.from_row_index, input.to_row_index);
    let expected = expected_route_sides(input.from_rect, input.to_rect, lane, row);
    RouteIntent {
        relationship_id: input.relationship.id.clone(),
        role: relationship_role(input.relationship),
        return_of: input.relationship.return_of.clone(),
        outcome: input.relationship.outcome.clone(),
        lane_direction: lane.to_string(),
        row_direction: row.to_string(),
        expected_source_side: expected.source,
        expected_target_side: expected.target,
    }
}

/// Port of JS `semanticSurfaceOptions({ expectedSides, relationship, fromRect, toRect, blockerRects, canvasWidth, canvasHeight })`.
pub fn semantic_surface_options(input: &SemanticSurfaceOptionsInput<'_>) -> SurfaceOptions {
    let mut source: IndexSet<String> = IndexSet::new();
    let mut target: IndexSet<String> = IndexSet::new();
    source.insert(input.expected_sides.source.clone());
    target.insert(input.expected_sides.target.clone());

    let horizontal_intent = (input.expected_sides.source == "right"
        && input.expected_sides.target == "left")
        || (input.expected_sides.source == "left"
            && input.expected_sides.target == "right");
    let vertical_intent = (input.expected_sides.source == "bottom"
        && input.expected_sides.target == "top")
        || (input.expected_sides.source == "top"
            && input.expected_sides.target == "bottom");

    // Vertical gutter escape only when at least 1 blocker in corridor
    let vertical_gutter_escape = vertical_intent
        && corridor_blocker_count(input.from_rect, input.to_rect, &input.blocker_rects) >= 1;

    if (horizontal_intent || vertical_gutter_escape)
        && semantic_primary_corridor_blocked(
            input.relationship,
            input.from_rect,
            input.to_rect,
            &input.blocker_rects,
            &input.expected_sides,
        )
    {
        let source_escape = escape_side_for(
            input.from_rect,
            &input.expected_sides.source,
            input.canvas_width,
            input.canvas_height,
        );
        let target_escape = escape_side_for(
            input.to_rect,
            &input.expected_sides.target,
            input.canvas_width,
            input.canvas_height,
        );
        // JS: relationship?.kind === "return" || Boolean(relationship?.returnOf)
        let is_return = input.relationship.kind.as_deref() == Some("return")
            || input.relationship.return_of.is_some();

        // Coplanar: same row for horizontal intent, same column for vertical intent
        let coplanar = if horizontal_intent {
            input.from_rect.y == input.to_rect.y
        } else {
            input.from_rect.x == input.to_rect.x
        };

        if coplanar {
            if !source_escape.is_empty() {
                source.insert(source_escape);
            }
            if !target_escape.is_empty() {
                target.insert(target_escape);
            }
        } else {
            // Non-coplanar: escape the arriving end only
            if is_return && !source_escape.is_empty() {
                source.insert(source_escape);
            }
            if !is_return && !target_escape.is_empty() {
                target.insert(target_escape);
            }
        }
    }

    SurfaceOptions { source, target }
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

    fn rel(id: &str) -> IntentRelationship {
        IntentRelationship { id: id.to_string(), ..Default::default() }
    }

    fn semantic_rel(id: &str) -> IntentRelationship {
        IntentRelationship {
            id: id.to_string(),
            kind: Some("process".to_string()),
            relationship_type: Some("dependency".to_string()),
            ..Default::default()
        }
    }

    fn return_rel(id: &str, return_of: &str) -> IntentRelationship {
        IntentRelationship {
            id: id.to_string(),
            kind: Some("return".to_string()),
            relationship_type: Some("dependency".to_string()),
            return_of: Some(return_of.to_string()),
            ..Default::default()
        }
    }

    // --- expectedFacingSides ---

    #[test]
    fn facing_sides_horizontal_right() {
        // Node: expectedFacingSides(f1,f2) → source:right, target:left
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let s = expected_facing_sides(&f1, &f2);
        assert_eq!(s.source, "right");
        assert_eq!(s.target, "left");
    }

    #[test]
    fn facing_sides_horizontal_left() {
        // Node: expectedFacingSides(f2,f1) → source:left, target:right
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let s = expected_facing_sides(&f2, &f1);
        assert_eq!(s.source, "left");
        assert_eq!(s.target, "right");
    }

    #[test]
    fn facing_sides_vertical_down() {
        // Node: expectedFacingSides(f1,f3) → source:bottom, target:top
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f3 = rect(0.0, 200.0, 80.0, 40.0);
        let s = expected_facing_sides(&f1, &f3);
        assert_eq!(s.source, "bottom");
        assert_eq!(s.target, "top");
    }

    #[test]
    fn facing_sides_vertical_up() {
        // Node: expectedFacingSides(f3,f1) → source:top, target:bottom
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f3 = rect(0.0, 200.0, 80.0, 40.0);
        let s = expected_facing_sides(&f3, &f1);
        assert_eq!(s.source, "top");
        assert_eq!(s.target, "bottom");
    }

    #[test]
    fn facing_sides_equal_abs_horizontal_wins() {
        // Node: eq1/eq2 (dx=100,dy=100) → >= so horizontal: source:right, target:left
        let eq1 = rect(0.0, 0.0, 100.0, 100.0);
        let eq2 = rect(100.0, 100.0, 100.0, 100.0);
        let s = expected_facing_sides(&eq1, &eq2);
        assert_eq!(s.source, "right");
        assert_eq!(s.target, "left");
    }

    // --- expectedRouteSides ---

    #[test]
    fn route_sides_forward() {
        // Node: forward → right/left
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let s = expected_route_sides(&f1, &f2, "forward", "same");
        assert_eq!(s.source, "right");
        assert_eq!(s.target, "left");
    }

    #[test]
    fn route_sides_backward() {
        // Node: backward → left/right
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let s = expected_route_sides(&f1, &f2, "backward", "same");
        assert_eq!(s.source, "left");
        assert_eq!(s.target, "right");
    }

    #[test]
    fn route_sides_down() {
        // Node: down → bottom/top
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f3 = rect(0.0, 200.0, 80.0, 40.0);
        let s = expected_route_sides(&f1, &f3, "same", "down");
        assert_eq!(s.source, "bottom");
        assert_eq!(s.target, "top");
    }

    #[test]
    fn route_sides_up() {
        // Node: up → top/bottom
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f3 = rect(0.0, 200.0, 80.0, 40.0);
        let s = expected_route_sides(&f3, &f1, "same", "up");
        assert_eq!(s.source, "top");
        assert_eq!(s.target, "bottom");
    }

    #[test]
    fn route_sides_same_same_falls_through_to_facing() {
        // Node: same/same → expectedFacingSides → right/left (f1 left of f2)
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let s = expected_route_sides(&f1, &f2, "same", "same");
        assert_eq!(s.source, "right");
        assert_eq!(s.target, "left");
    }

    // --- deriveRouteIntent ---

    #[test]
    fn derive_intent_forward_process() {
        // Node: {relationshipId:"r1", role:"process", laneDirection:"forward",
        //        rowDirection:"same", expectedSourceSide:"right", expectedTargetSide:"left"}
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let r = IntentRelationship {
            id: "r1".to_string(),
            kind: Some("process".to_string()),
            ..Default::default()
        };
        let result = derive_route_intent(&DeriveRouteIntentInput {
            relationship: &r,
            from_rect: &f1,
            to_rect: &f2,
            from_lane_index: 0,
            to_lane_index: 1,
            from_row_index: 0,
            to_row_index: 0,
        });
        assert_eq!(result.relationship_id, "r1");
        assert_eq!(result.role, "process");
        assert_eq!(result.lane_direction, "forward");
        assert_eq!(result.row_direction, "same");
        assert_eq!(result.expected_source_side, "right");
        assert_eq!(result.expected_target_side, "left");
        assert!(result.return_of.is_none());
    }

    #[test]
    fn derive_intent_backward_return() {
        // Node: role:"return", returnOf:"r1", backward/same → left/right
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let r = IntentRelationship {
            id: "r2".to_string(),
            return_of: Some("r1".to_string()),
            ..Default::default()
        };
        let result = derive_route_intent(&DeriveRouteIntentInput {
            relationship: &r,
            from_rect: &f1,
            to_rect: &f2,
            from_lane_index: 1,
            to_lane_index: 0,
            from_row_index: 0,
            to_row_index: 0,
        });
        assert_eq!(result.role, "return");
        assert_eq!(result.return_of, Some("r1".to_string()));
        assert_eq!(result.lane_direction, "backward");
        assert_eq!(result.expected_source_side, "left");
        assert_eq!(result.expected_target_side, "right");
    }

    #[test]
    fn derive_intent_down_decision_outcome() {
        // Node: role:"decision-outcome", outcome:"success", down → bottom/top
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f3 = rect(0.0, 200.0, 80.0, 40.0);
        let r = IntentRelationship {
            id: "r3".to_string(),
            outcome: Some("success".to_string()),
            ..Default::default()
        };
        let result = derive_route_intent(&DeriveRouteIntentInput {
            relationship: &r,
            from_rect: &f1,
            to_rect: &f3,
            from_lane_index: 0,
            to_lane_index: 0,
            from_row_index: 0,
            to_row_index: 1,
        });
        assert_eq!(result.role, "decision-outcome");
        assert_eq!(result.outcome, Some("success".to_string()));
        assert_eq!(result.row_direction, "down");
        assert_eq!(result.expected_source_side, "bottom");
        assert_eq!(result.expected_target_side, "top");
    }

    #[test]
    fn derive_intent_no_kind_defaults_to_process() {
        // Node: no kind/returnOf/outcome → role="process"
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let r = rel("r4");
        let result = derive_route_intent(&DeriveRouteIntentInput {
            relationship: &r,
            from_rect: &f1,
            to_rect: &f2,
            from_lane_index: 0,
            to_lane_index: 0,
            from_row_index: 0,
            to_row_index: 0,
        });
        assert_eq!(result.role, "process");
        assert_eq!(result.lane_direction, "same");
        assert_eq!(result.row_direction, "same");
    }

    // --- semanticSurfaceOptions ---

    #[test]
    fn semantic_surface_blocked_horizontal_coplanar() {
        // Node: blocked coplanar → source:["right","bottom"], target:["left","bottom"]
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let blocker = rect(90.0, 0.0, 80.0, 40.0);
        let r = semantic_rel("r1");
        let result = semantic_surface_options(&SemanticSurfaceOptionsInput {
            expected_sides: SidePair { source: "right".to_string(), target: "left".to_string() },
            relationship: &r,
            from_rect: &f1,
            to_rect: &f2,
            blocker_rects: vec![blocker],
            canvas_width: 500.0,
            canvas_height: 300.0,
        });
        let src: Vec<&str> = result.source.iter().map(|s| s.as_str()).collect();
        let tgt: Vec<&str> = result.target.iter().map(|s| s.as_str()).collect();
        assert_eq!(src, vec!["right", "bottom"]);
        assert_eq!(tgt, vec!["left", "bottom"]);
    }

    #[test]
    fn semantic_surface_unblocked() {
        // Node: no blocker → source:["right"], target:["left"]
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let r = semantic_rel("r1");
        let result = semantic_surface_options(&SemanticSurfaceOptionsInput {
            expected_sides: SidePair { source: "right".to_string(), target: "left".to_string() },
            relationship: &r,
            from_rect: &f1,
            to_rect: &f2,
            blocker_rects: vec![],
            canvas_width: 500.0,
            canvas_height: 300.0,
        });
        let src: Vec<&str> = result.source.iter().map(|s| s.as_str()).collect();
        let tgt: Vec<&str> = result.target.iter().map(|s| s.as_str()).collect();
        assert_eq!(src, vec!["right"]);
        assert_eq!(tgt, vec!["left"]);
    }

    #[test]
    fn semantic_surface_empty_relationship_no_escape() {
        // Node: rel with no semantic fields → semanticPrimaryCorridorBlocked=false → no escape
        let f1 = rect(0.0, 0.0, 80.0, 40.0);
        let f2 = rect(200.0, 0.0, 80.0, 40.0);
        let r = rel("r5"); // no kind/returnOf/outcome/relationshipType/stepId/flowId
        let blocker = rect(90.0, 0.0, 80.0, 40.0);
        let result = semantic_surface_options(&SemanticSurfaceOptionsInput {
            expected_sides: SidePair { source: "right".to_string(), target: "left".to_string() },
            relationship: &r,
            from_rect: &f1,
            to_rect: &f2,
            blocker_rects: vec![blocker],
            canvas_width: 500.0,
            canvas_height: 300.0,
        });
        let src: Vec<&str> = result.source.iter().map(|s| s.as_str()).collect();
        let tgt: Vec<&str> = result.target.iter().map(|s| s.as_str()).collect();
        // Node: source:["right"], target:["left"]
        assert_eq!(src, vec!["right"]);
        assert_eq!(tgt, vec!["left"]);
    }

    #[test]
    fn semantic_surface_vertical_blocked_coplanar() {
        // Node: vertical blocked → source:["bottom","left"], target:["top","left"]
        let from = rect(100.0, 0.0, 80.0, 40.0);
        let to = rect(100.0, 200.0, 80.0, 40.0);
        let blocker = rect(80.0, 60.0, 80.0, 60.0);
        let r = semantic_rel("r1");
        let result = semantic_surface_options(&SemanticSurfaceOptionsInput {
            expected_sides: SidePair { source: "bottom".to_string(), target: "top".to_string() },
            relationship: &r,
            from_rect: &from,
            to_rect: &to,
            blocker_rects: vec![blocker],
            canvas_width: 500.0,
            canvas_height: 300.0,
        });
        let src: Vec<&str> = result.source.iter().map(|s| s.as_str()).collect();
        let tgt: Vec<&str> = result.target.iter().map(|s| s.as_str()).collect();
        assert_eq!(src, vec!["bottom", "left"]);
        assert_eq!(tgt, vec!["top", "left"]);
    }

    #[test]
    fn semantic_surface_return_non_coplanar_escapes_source() {
        // Node: return rel, non-coplanar → source escapes, target stays
        // source:["right","bottom"], target:["left"]
        let ret_from = rect(0.0, 0.0, 80.0, 40.0);
        let ret_to = rect(200.0, 50.0, 80.0, 40.0);
        let ret_blocker = rect(90.0, 10.0, 60.0, 30.0);
        let r = return_rel("r6", "r5");
        let result = semantic_surface_options(&SemanticSurfaceOptionsInput {
            expected_sides: SidePair { source: "right".to_string(), target: "left".to_string() },
            relationship: &r,
            from_rect: &ret_from,
            to_rect: &ret_to,
            blocker_rects: vec![ret_blocker],
            canvas_width: 500.0,
            canvas_height: 300.0,
        });
        let src: Vec<&str> = result.source.iter().map(|s| s.as_str()).collect();
        let tgt: Vec<&str> = result.target.iter().map(|s| s.as_str()).collect();
        assert_eq!(src, vec!["right", "bottom"]);
        assert_eq!(tgt, vec!["left"]);
    }
}
