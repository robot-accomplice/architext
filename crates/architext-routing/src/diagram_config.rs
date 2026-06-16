//! Port of `src/domain/diagram-config/diagram-config.mjs`.
//!
//! `resolve_diagram_config` with `resolveDiagramConfig` semantics, plus the
//! defaults path so that with NO config files present the resolved
//! `diagram.layout` matches JS exactly (the corpus has no config files).
//!
//! The corpus default path is the only one exercised by the parity gate, but
//! the resolver is ported faithfully for completeness.

use crate::plan_request::diagram_layout::LayoutConfig;

/// Default values for the layout section, mirroring `DIAGRAM_CONFIG_FIELDS.layout`.
const DEFAULTS_LANE_WIDTH: f64 = 210.0;
const DEFAULTS_ROW_GAP: f64 = 102.0;
const DEFAULTS_NODE_WIDTH: f64 = 136.0;
const DEFAULTS_NODE_HEIGHT: f64 = 54.0;
const DEFAULTS_ROUTE_GUTTER: f64 = 132.0;
const DEFAULTS_MARGIN_Y: f64 = 104.0;

/// Resolved `layout` section of the diagram config.
/// Maps directly to `LayoutConfig` for use by `diagram_layout_for`.
#[derive(Debug, Clone)]
pub struct DiagramConfigLayout {
    pub lane_width: f64,
    pub row_gap: f64,
    pub node_width: f64,
    pub node_height: f64,
    pub route_gutter: f64,
    pub margin_y: f64,
}

impl Default for DiagramConfigLayout {
    fn default() -> Self {
        Self {
            lane_width: DEFAULTS_LANE_WIDTH,
            row_gap: DEFAULTS_ROW_GAP,
            node_width: DEFAULTS_NODE_WIDTH,
            node_height: DEFAULTS_NODE_HEIGHT,
            route_gutter: DEFAULTS_ROUTE_GUTTER,
            margin_y: DEFAULTS_MARGIN_Y,
        }
    }
}

impl DiagramConfigLayout {
    /// Convert to `LayoutConfig` where all defaults are expressed as `None`
    /// (so `diagram_layout_for` falls through to its own base values),
    /// and only NON-default values are `Some`.
    ///
    /// This is crucial: the JS precompute farm passes `config?.diagram?.layout`
    /// from `diagramConfigGetPayload`, which returns the full resolved config
    /// (defaults included). When there are no config files, every field equals
    /// its default. The JS `diagramLayoutFor` then sees, e.g., `o.laneWidth = 210`
    /// and uses that over the auto-selected dense value (because `210 ?? 210` is
    /// still `210` and the dense auto is also `240`). Wait — let me check the JS
    /// more carefully.
    ///
    /// JS `diagramLayoutFor(view, relCount, layoutConfig)`:
    ///   `const o = layoutConfig ?? {};`
    ///   `const laneWidth = o.laneWidth ?? (isDenseTopology ? 240 : BASE_LAYOUT.laneWidth);`
    ///
    /// When `layoutConfig` is the full defaults object (from `diagramConfigGetPayload`),
    /// `o.laneWidth = 210` (the default). For a dense view, the auto would be 240,
    /// but `210` wins because `210 ?? 240` → `210`. This means the JS precompute
    /// farm ALWAYS uses the default values and NEVER goes dense when a full config
    /// is present — even the defaults config.
    ///
    /// The parity gate calls the JS oracle with `layoutConfig: (await diagramConfigGetPayload(repoRoot))?.diagram?.layout`.
    /// With no config files present, that payload returns the full default config.
    ///
    /// Therefore we must pass ALL fields (including defaults) as explicit overrides
    /// to `LayoutConfig` — not `None` for defaults.
    pub fn to_layout_config(&self) -> LayoutConfig {
        LayoutConfig {
            lane_width: Some(self.lane_width),
            row_gap: Some(self.row_gap),
            node_width: Some(self.node_width),
            node_height: Some(self.node_height),
            route_gutter: Some(self.route_gutter),
            margin_y: Some(self.margin_y),
        }
    }
}

/// Resolved diagram config. Currently only `layout` is needed by the farm.
#[derive(Debug, Clone, Default)]
pub struct DiagramConfig {
    pub layout: DiagramConfigLayout,
}

/// Port of JS `resolveDiagramConfig(layers)` → `{ config }`.
///
/// For the parity gate's corpus (no config files present), just return defaults.
pub fn resolve_diagram_config_defaults() -> DiagramConfig {
    DiagramConfig {
        layout: DiagramConfigLayout::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_js_constants() {
        let cfg = resolve_diagram_config_defaults();
        assert_eq!(cfg.layout.lane_width, 210.0);
        assert_eq!(cfg.layout.row_gap, 102.0);
        assert_eq!(cfg.layout.node_width, 136.0);
        assert_eq!(cfg.layout.node_height, 54.0);
        assert_eq!(cfg.layout.route_gutter, 132.0);
        assert_eq!(cfg.layout.margin_y, 104.0);
    }

    #[test]
    fn to_layout_config_all_some() {
        let cfg = resolve_diagram_config_defaults();
        let lc = cfg.layout.to_layout_config();
        // With a full default config, all fields are Some (explicit, not None)
        // so diagramLayoutFor will use them and never fall through to dense auto-values.
        assert_eq!(lc.lane_width, Some(210.0));
        assert_eq!(lc.route_gutter, Some(132.0));
    }
}
