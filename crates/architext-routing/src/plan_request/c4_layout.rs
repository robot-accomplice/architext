//! Port of `viewer/src/routing/c4Layout.js`.
//!
//! C4 views (`c4-context`/`c4-container`/`c4-component`) use type-specific
//! layout dimensions rather than the auto-selected `diagram_layout_for` values.
//! `c4_layout_for(view_type)` returns the matching dimensions, falling back to
//! the `c4-container` profile for any unrecognised C4 type (matching the JS
//! `c4Layouts[viewType] ?? c4Layouts["c4-container"]`).
//!
//! These layouts feed the SAME assemble/plan pipeline as flows — only the
//! dimension numbers differ. `route_gutter` is not a C4 layout concept (the JS
//! C4 layouts omit it); `margin_x` is an explicit per-type value here, so the
//! resulting `DiagramLayout.route_gutter` is set to `margin_x` purely to keep the
//! struct populated — it is unused by `plan_diagram`, which derives canvas extent
//! from `margin_x`/`canvas_extra_*` directly.

use crate::plan_request::diagram_layout::DiagramLayout;

/// One C4 layout profile — the exact field set the JS `c4Layouts` table carries
/// (minus `boundaryLabel`, which is a render-only string the routing engine
/// never consumes).
struct C4Profile {
    node_width: f64,
    node_height: f64,
    lane_width: f64,
    row_gap: f64,
    margin_x: f64,
    margin_y: f64,
    min_canvas_width: f64,
    min_canvas_height: f64,
    canvas_extra_width: f64,
    canvas_extra_height: f64,
}

const C4_CONTEXT: C4Profile = C4Profile {
    node_width: 184.0,
    node_height: 96.0,
    lane_width: 280.0,
    row_gap: 144.0,
    margin_x: 104.0,
    margin_y: 104.0,
    min_canvas_width: 920.0,
    min_canvas_height: 500.0,
    canvas_extra_width: 112.0,
    canvas_extra_height: 120.0,
};

const C4_CONTAINER: C4Profile = C4Profile {
    node_width: 176.0,
    node_height: 104.0,
    lane_width: 270.0,
    row_gap: 156.0,
    margin_x: 104.0,
    margin_y: 108.0,
    min_canvas_width: 960.0,
    min_canvas_height: 540.0,
    canvas_extra_width: 128.0,
    canvas_extra_height: 128.0,
};

const C4_COMPONENT: C4Profile = C4Profile {
    node_width: 168.0,
    node_height: 98.0,
    lane_width: 252.0,
    row_gap: 140.0,
    margin_x: 96.0,
    margin_y: 104.0,
    min_canvas_width: 900.0,
    min_canvas_height: 520.0,
    canvas_extra_width: 112.0,
    canvas_extra_height: 120.0,
};

/// Port of JS `c4LayoutFor(viewType)`.
///
/// `c4-context`/`c4-container`/`c4-component` map to their profiles; any other
/// type (including `c4-code`, which the JS table has no entry for) falls back to
/// the `c4-container` profile.
pub fn c4_layout_for(view_type: &str) -> DiagramLayout {
    let p = match view_type {
        "c4-context" => &C4_CONTEXT,
        "c4-component" => &C4_COMPONENT,
        // "c4-container" and any unrecognised C4 type → container fallback.
        _ => &C4_CONTAINER,
    };
    DiagramLayout {
        node_width: p.node_width,
        node_height: p.node_height,
        lane_width: p.lane_width,
        row_gap: p.row_gap,
        // Not a C4 layout concept; kept equal to margin_x so the struct is fully
        // populated. `plan_diagram` ignores `route_gutter`.
        route_gutter: p.margin_x,
        margin_x: p.margin_x,
        margin_y: p.margin_y,
        min_canvas_width: p.min_canvas_width,
        min_canvas_height: p.min_canvas_height,
        canvas_extra_width: p.canvas_extra_width,
        canvas_extra_height: p.canvas_extra_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_dims_match_js() {
        let l = c4_layout_for("c4-context");
        assert_eq!(l.node_width, 184.0);
        assert_eq!(l.lane_width, 280.0);
        assert_eq!(l.row_gap, 144.0);
        assert_eq!(l.margin_x, 104.0);
        assert_eq!(l.min_canvas_width, 920.0);
        assert_eq!(l.canvas_extra_width, 112.0);
    }

    #[test]
    fn container_dims_match_js() {
        let l = c4_layout_for("c4-container");
        assert_eq!(l.node_width, 176.0);
        assert_eq!(l.node_height, 104.0);
        assert_eq!(l.row_gap, 156.0);
        assert_eq!(l.min_canvas_height, 540.0);
        assert_eq!(l.canvas_extra_height, 128.0);
    }

    #[test]
    fn component_dims_match_js() {
        let l = c4_layout_for("c4-component");
        assert_eq!(l.node_width, 168.0);
        assert_eq!(l.lane_width, 252.0);
        assert_eq!(l.margin_x, 96.0);
    }

    #[test]
    fn code_and_unknown_fall_back_to_container() {
        // JS: c4Layouts["c4-code"] is undefined → "?? c4Layouts['c4-container']".
        let code = c4_layout_for("c4-code");
        let container = c4_layout_for("c4-container");
        assert_eq!(code.node_width, container.node_width);
        assert_eq!(code.lane_width, container.lane_width);
        assert_eq!(c4_layout_for("nonsense").node_width, container.node_width);
    }
}
