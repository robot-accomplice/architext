//! Faithful port of `viewer/src/routing/routeLabels.js`.
//!
//! Translation decisions:
//! - `routeLength` imported from `crate::route_geometry`.
//! - `relationship.label ?? relationship.id ?? ""` → Rust option-chaining with
//!   empty-string fallback; `LabelRelationship` carries optional `label`/`id`.
//! - `Math.max(24, Math.min(180, text.length * 6 + 12))` — `text.length` in JS
//!   is the UTF-16 code unit count. For ASCII routing ids/labels this equals the
//!   byte/char count, which covers all real call sites. We use `str::len()` (byte
//!   count), which agrees with UTF-16 for ASCII. Non-ASCII would diverge; there
//!   are no non-ASCII labels in the router, so this is the faithful translation.
//! - `route.points.every(p => p.x === start.x)` / `.every(p => p.y === start.y)`
//!   — strict f64 equality, matching JS `===`.
//! - Spread operator `{ ...route, labelX: ... }` → clone + field update.

use crate::model::Point;
use crate::route_geometry::route_length;

// ---------------------------------------------------------------------------
// Label box estimation input types
// ---------------------------------------------------------------------------

/// Subset of the relationship object needed by `estimatedLabelBox`.
///
/// - `relationship_type`: JS `relationship.relationshipType`
/// - `step_id`: JS `relationship.stepId`
/// - `label`: JS `relationship.label`
/// - `id`: JS `relationship.id`
#[derive(Debug, Clone, Default)]
pub struct LabelRelationship {
    pub relationship_type: Option<String>,
    pub step_id: Option<String>,
    pub label: Option<String>,
    pub id: Option<String>,
}

// ---------------------------------------------------------------------------
// Rect for label boxes
// ---------------------------------------------------------------------------

/// A label bounding box: `{ x, y, width, height }`.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// ---------------------------------------------------------------------------
// estimatedLabelBox
// ---------------------------------------------------------------------------

/// Port of JS `estimatedLabelBox(labelPoint, relationship)`.
///
/// Returns `None` when `relationship` is absent (JS `null`/`undefined` → `null`).
/// Flow/step relationships get a fixed 28×24 box centred on `labelPoint`.
/// All others size by text width, clamped to [24, 180] px.
pub fn estimated_label_box(label_point: &Point, relationship: Option<&LabelRelationship>) -> Option<LabelBox> {
    let rel = relationship?;

    if rel.relationship_type.as_deref() == Some("flow") || rel.step_id.is_some() {
        return Some(LabelBox {
            x: label_point.x - 14.0,
            y: label_point.y - 12.0,
            width: 28.0,
            height: 24.0,
        });
    }

    // `relationship.label ?? relationship.id ?? ""`
    let text = rel
        .label
        .as_deref()
        .or(rel.id.as_deref())
        .unwrap_or("");

    // JS: Math.max(24, Math.min(180, text.length * 6 + 12))
    // text.length in JS = UTF-16 code unit count; for ASCII ids == byte count.
    // Written as nested max/min to faithfully mirror JS; clippy's .clamp() is
    // equivalent for finite values but we want an explicit JS-match marker here.
    #[allow(clippy::manual_clamp)]
    let width = f64::max(24.0, f64::min(180.0, text.len() as f64 * 6.0 + 12.0));

    Some(LabelBox {
        x: label_point.x - width / 2.0,
        y: label_point.y - 9.0,
        width,
        height: 18.0,
    })
}

// ---------------------------------------------------------------------------
// Route shape for withReadableLabel
// ---------------------------------------------------------------------------

/// Minimal route shape required by `withReadableLabel`.
///
/// Mirrors the fields the JS function reads from a route object:
/// - `points`: the polyline vertices
/// - `samples`: the sampled polyline (used for length measurement)
/// - `label_x` / `label_y`: the current label position (mutated on short routes)
#[derive(Debug, Clone, PartialEq)]
pub struct RouteForLabel {
    pub points: Vec<Point>,
    pub samples: Vec<Point>,
    pub label_x: f64,
    pub label_y: f64,
}

// ---------------------------------------------------------------------------
// withReadableLabel
// ---------------------------------------------------------------------------

/// Port of JS `withReadableLabel(route)`.
///
/// If the route length is ≥ 70 px, returns the route unchanged.
/// For short vertical routes (all points share the same x), shifts `labelX` +28.
/// For short horizontal routes (all points share the same y), shifts `labelY` -22.
/// Otherwise returns unchanged.
///
/// The JS `{ ...route }` spread is reproduced by cloning the input. Point
/// equality uses strict f64 `==`, matching JS `===`.
pub fn with_readable_label(route: &RouteForLabel) -> RouteForLabel {
    let length = route_length(&route.samples);
    if length >= 70.0 {
        return route.clone();
    }

    let start = match route.points.first() {
        Some(p) => p,
        None => return route.clone(),
    };

    let is_vertical = route.points.iter().all(|p| p.x == start.x);
    let is_horizontal = route.points.iter().all(|p| p.y == start.y);

    if is_vertical {
        let mut r = route.clone();
        r.label_x += 28.0;
        return r;
    }
    if is_horizontal {
        let mut r = route.clone();
        r.label_y -= 22.0;
        return r;
    }
    route.clone()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn flow_rel() -> LabelRelationship {
        LabelRelationship {
            relationship_type: Some("flow".into()),
            id: Some("r1".into()),
            label: Some("some flow".into()),
            ..Default::default()
        }
    }

    fn step_rel() -> LabelRelationship {
        LabelRelationship {
            step_id: Some("s1".into()),
            id: Some("r2".into()),
            label: Some("step rel".into()),
            ..Default::default()
        }
    }

    fn normal_rel() -> LabelRelationship {
        // label = "hello world" → length=11 → width = max(24,min(180,11*6+12)) = max(24,78)=78
        LabelRelationship {
            id: Some("r3".into()),
            label: Some("hello world".into()),
            ..Default::default()
        }
    }

    fn short_rel() -> LabelRelationship {
        // label = "hi" → length=2 → width = max(24, min(180,2*6+12)) = max(24,24)=24
        LabelRelationship {
            id: Some("r4".into()),
            label: Some("hi".into()),
            ..Default::default()
        }
    }

    fn no_label_rel() -> LabelRelationship {
        // no label, id="r5" → text="r5" → length=2 → width=24
        LabelRelationship {
            id: Some("r5".into()),
            ..Default::default()
        }
    }

    fn long_rel() -> LabelRelationship {
        // Very long label → capped at 180
        // "this is a very very very long label that exceeds 180px width cap calculation"
        LabelRelationship {
            id: Some("r6".into()),
            label: Some("this is a very very very long label that exceeds 180px width cap calculation".into()),
            ..Default::default()
        }
    }

    // --- estimatedLabelBox ---

    #[test]
    fn flow_rel_fixed_box() {
        // Node: flow rel → {x:86,y:38,width:28,height:24}
        let b = estimated_label_box(&pt(100.0, 50.0), Some(&flow_rel())).unwrap();
        assert_eq!(b.x, 86.0);
        assert_eq!(b.y, 38.0);
        assert_eq!(b.width, 28.0);
        assert_eq!(b.height, 24.0);
    }

    #[test]
    fn step_rel_fixed_box() {
        // Node: step rel → {x:86,y:38,width:28,height:24}
        let b = estimated_label_box(&pt(100.0, 50.0), Some(&step_rel())).unwrap();
        assert_eq!(b.x, 86.0);
        assert_eq!(b.y, 38.0);
        assert_eq!(b.width, 28.0);
        assert_eq!(b.height, 24.0);
    }

    #[test]
    fn normal_rel_label_width() {
        // Node: {x:61,y:41,width:78,height:18}
        let b = estimated_label_box(&pt(100.0, 50.0), Some(&normal_rel())).unwrap();
        assert_eq!(b.x, 61.0);
        assert_eq!(b.y, 41.0);
        assert_eq!(b.width, 78.0);
        assert_eq!(b.height, 18.0);
    }

    #[test]
    fn short_rel_min_width() {
        // Node: {x:88,y:41,width:24,height:18}
        let b = estimated_label_box(&pt(100.0, 50.0), Some(&short_rel())).unwrap();
        assert_eq!(b.x, 88.0);
        assert_eq!(b.y, 41.0);
        assert_eq!(b.width, 24.0);
        assert_eq!(b.height, 18.0);
    }

    #[test]
    fn no_label_uses_id() {
        // Node: no label rel → {x:88,y:41,width:24,height:18}
        let b = estimated_label_box(&pt(100.0, 50.0), Some(&no_label_rel())).unwrap();
        assert_eq!(b.width, 24.0);
        assert_eq!(b.height, 18.0);
    }

    #[test]
    fn none_rel_returns_none() {
        // Node: null/undefined rel → null
        assert!(estimated_label_box(&pt(100.0, 50.0), None).is_none());
    }

    #[test]
    fn long_rel_capped_at_180() {
        // Node: {x:10,y:41,width:180,height:18}
        let b = estimated_label_box(&pt(100.0, 50.0), Some(&long_rel())).unwrap();
        assert_eq!(b.width, 180.0);
        assert_eq!(b.height, 18.0);
    }

    // --- withReadableLabel ---

    #[test]
    fn long_route_unchanged() {
        // route length = 90 >= 70 → no modification
        let route = RouteForLabel {
            points: vec![pt(10.0, 10.0), pt(10.0, 100.0)],
            samples: vec![pt(10.0, 10.0), pt(10.0, 100.0)],
            label_x: 50.0,
            label_y: 50.0,
        };
        let result = with_readable_label(&route);
        // Node: unchanged (length=90)
        assert_eq!(result.label_x, 50.0);
        assert_eq!(result.label_y, 50.0);
    }

    #[test]
    fn short_vertical_route_shifts_label_x() {
        // route: [10,10]→[10,40], length=30 < 70, all points x=10 → isVertical
        // Node: labelX += 28 → 78
        let route = RouteForLabel {
            points: vec![pt(10.0, 10.0), pt(10.0, 40.0)],
            samples: vec![pt(10.0, 10.0), pt(10.0, 40.0)],
            label_x: 50.0,
            label_y: 50.0,
        };
        let result = with_readable_label(&route);
        assert_eq!(result.label_x, 78.0);
        assert_eq!(result.label_y, 50.0);
    }

    #[test]
    fn short_horizontal_route_shifts_label_y() {
        // route: [10,10]→[40,10], length=30 < 70, all points y=10 → isHorizontal
        // Node: labelY -= 22 → 28
        let route = RouteForLabel {
            points: vec![pt(10.0, 10.0), pt(40.0, 10.0)],
            samples: vec![pt(10.0, 10.0), pt(40.0, 10.0)],
            label_x: 50.0,
            label_y: 50.0,
        };
        let result = with_readable_label(&route);
        assert_eq!(result.label_x, 50.0);
        assert_eq!(result.label_y, 28.0);
    }

    #[test]
    fn short_diagonal_route_unchanged() {
        // route: [10,10]→[30,30], not vertical, not horizontal → unchanged
        // Node: unchanged
        let route = RouteForLabel {
            points: vec![pt(10.0, 10.0), pt(30.0, 30.0)],
            samples: vec![pt(10.0, 10.0), pt(30.0, 30.0)],
            label_x: 50.0,
            label_y: 50.0,
        };
        let result = with_readable_label(&route);
        assert_eq!(result.label_x, 50.0);
        assert_eq!(result.label_y, 50.0);
    }

    #[test]
    fn long_horizontal_route_unchanged() {
        // route: [10,10]→[200,10], length=190 >= 70 → unchanged
        let route = RouteForLabel {
            points: vec![pt(10.0, 10.0), pt(200.0, 10.0)],
            samples: vec![pt(10.0, 10.0), pt(200.0, 10.0)],
            label_x: 50.0,
            label_y: 50.0,
        };
        let result = with_readable_label(&route);
        assert_eq!(result.label_x, 50.0);
        assert_eq!(result.label_y, 50.0);
    }
}
