//! Port of `viewer/src/presentation/diagramLayout.js`.
//!
//! `diagram_layout_for` computes the effective layout parameters for a view,
//! applying optional user overrides from the diagram config on top of the
//! auto-selected (default vs dense) base values.

use crate::plan_request::types::View;

/// Base layout constants — mirrors `BASE_LAYOUT` in diagramLayout.js exactly.
const NODE_WIDTH: f64 = 136.0;
const NODE_HEIGHT: f64 = 54.0;
const LANE_WIDTH: f64 = 210.0;
const ROW_GAP: f64 = 102.0;
const ROUTE_GUTTER: f64 = 132.0;
const MARGIN_Y: f64 = 104.0;
const MIN_CANVAS_WIDTH: f64 = 0.0;
const MIN_CANVAS_HEIGHT: f64 = 340.0;
const CANVAS_EXTRA_HEIGHT: f64 = 88.0;

/// Dense-topology view types that trigger wider spacing when they have many rows or relationships.
const DENSE_VIEW_TYPES: &[&str] = &["dataflow", "deployment", "flow-explorer", "risk-overlay"];

/// Resolved diagram layout values.
#[derive(Debug, Clone)]
pub struct DiagramLayout {
    pub node_width: f64,
    pub node_height: f64,
    pub lane_width: f64,
    pub row_gap: f64,
    pub route_gutter: f64,
    pub margin_x: f64,
    pub margin_y: f64,
    pub min_canvas_width: f64,
    pub min_canvas_height: f64,
    pub canvas_extra_width: f64,
    pub canvas_extra_height: f64,
}

/// Optional user overrides from the diagram config `layout` section.
#[derive(Debug, Clone, Default)]
pub struct LayoutConfig {
    pub node_width: Option<f64>,
    pub node_height: Option<f64>,
    pub lane_width: Option<f64>,
    pub row_gap: Option<f64>,
    pub route_gutter: Option<f64>,
    pub margin_y: Option<f64>,
}

/// Port of JS `diagramLayoutFor(view, relationshipCount, layoutConfig)`.
pub fn diagram_layout_for(
    view: &View,
    relationship_count: usize,
    layout_config: Option<&LayoutConfig>,
) -> DiagramLayout {
    let max_rows = view.lanes.iter()
        .map(|lane| lane.node_ids.len())
        .max()
        .unwrap_or(0)
        .max(1);

    let is_dense_topology = DENSE_VIEW_TYPES.contains(&view.view_type.as_str())
        && (max_rows >= 5 || relationship_count >= 8);

    let o = layout_config;
    let node_width = o.and_then(|c| c.node_width).unwrap_or(NODE_WIDTH);
    let node_height = o.and_then(|c| c.node_height).unwrap_or(NODE_HEIGHT);
    let lane_width = o.and_then(|c| c.lane_width).unwrap_or(if is_dense_topology { 240.0 } else { LANE_WIDTH });
    let row_gap = o.and_then(|c| c.row_gap).unwrap_or(if is_dense_topology { 176.0 } else { ROW_GAP });
    let route_gutter = o.and_then(|c| c.route_gutter).unwrap_or(if is_dense_topology { 180.0 } else { ROUTE_GUTTER });
    let margin_y = o.and_then(|c| c.margin_y).unwrap_or(MARGIN_Y);
    let min_canvas_height = if is_dense_topology { 560.0 } else { MIN_CANVAS_HEIGHT };

    DiagramLayout {
        node_width,
        node_height,
        lane_width,
        row_gap,
        route_gutter,
        margin_x: route_gutter + 48.0,
        margin_y,
        min_canvas_width: MIN_CANVAS_WIDTH,
        min_canvas_height,
        canvas_extra_width: route_gutter,
        canvas_extra_height: CANVAS_EXTRA_HEIGHT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_request::types::{Lane, View};

    fn mkview(view_type: &str, lane_sizes: &[usize]) -> View {
        View {
            id: "v1".to_string(),
            view_type: view_type.to_string(),
            lanes: lane_sizes.iter().enumerate().map(|(i, &n)| Lane {
                id: format!("l{i}"),
                node_ids: (0..n).map(|j| format!("n{i}{j}")).collect(),
            }).collect(),
        }
    }

    #[test]
    fn default_layout_system_map() {
        let view = mkview("system-map", &[3, 3, 3]);
        let layout = diagram_layout_for(&view, 5, None);
        assert_eq!(layout.node_width, 136.0);
        assert_eq!(layout.node_height, 54.0);
        assert_eq!(layout.lane_width, 210.0);
        assert_eq!(layout.row_gap, 102.0);
        assert_eq!(layout.route_gutter, 132.0);
        assert_eq!(layout.margin_x, 180.0); // 132 + 48
        assert_eq!(layout.margin_y, 104.0);
        assert_eq!(layout.min_canvas_height, 340.0);
        assert_eq!(layout.canvas_extra_width, 132.0);
        assert_eq!(layout.canvas_extra_height, 88.0);
    }

    #[test]
    fn dense_topology_dataflow_many_rows() {
        let view = mkview("dataflow", &[5, 5]); // max_rows=5 >= 5 → dense
        let layout = diagram_layout_for(&view, 3, None);
        assert_eq!(layout.lane_width, 240.0);
        assert_eq!(layout.row_gap, 176.0);
        assert_eq!(layout.route_gutter, 180.0);
        assert_eq!(layout.min_canvas_height, 560.0);
    }

    #[test]
    fn dense_topology_dataflow_many_relationships() {
        let view = mkview("dataflow", &[3]); // max_rows=3 < 5, but relationships=8 → dense
        let layout = diagram_layout_for(&view, 8, None);
        assert_eq!(layout.lane_width, 240.0);
    }

    #[test]
    fn layout_config_override_wins() {
        let view = mkview("system-map", &[3]);
        let config = LayoutConfig { lane_width: Some(300.0), ..Default::default() };
        let layout = diagram_layout_for(&view, 0, Some(&config));
        assert_eq!(layout.lane_width, 300.0);
        // non-overridden values unchanged
        assert_eq!(layout.node_width, 136.0);
    }
}
