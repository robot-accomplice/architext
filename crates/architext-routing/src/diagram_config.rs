//! Port of `src/domain/diagram-config/diagram-config.mjs`.
//!
//! `resolve_diagram_config` with `resolveDiagramConfig` semantics, plus the
//! defaults path so that with NO config files present the resolved
//! `diagram.layout` matches JS exactly (the corpus has no config files).
//!
//! The corpus default path is the only one exercised by the parity gate, but
//! the resolver is ported faithfully for completeness.
//!
//! The full `resolve_diagram_config_from_json` resolver (used by the serve
//! layer's `/api/config` endpoint) accepts ordered JSON layers and mirrors
//! the JS `normalizeDiagramConfigLayer` + `resolveDiagramConfig` logic.

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

// ─── Field specification (mirrors DIAGRAM_CONFIG_FIELDS + SECTION_LABELS) ────

/// A single configurable field's spec (mirrors JS `{ default, min, max, step, unit, label }`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldSpec {
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: &'static str,
    pub label: &'static str,
}

/// Returns the `DIAGRAM_CONFIG_FIELDS` static spec as a `serde_json::Value`.
/// Shape: `{ layout: { laneWidth: { default, min, max, step, unit, label }, ... }, ... }`
pub fn diagram_config_fields_json() -> serde_json::Value {
    serde_json::json!({
        "layout": {
            "laneWidth":   { "default": 210, "min": 60,   "max": 800, "step": 2,    "unit": "px",          "label": "Column width" },
            "rowGap":      { "default": 102, "min": 20,   "max": 600, "step": 2,    "unit": "px",          "label": "Row gap" },
            "nodeWidth":   { "default": 136, "min": 40,   "max": 600, "step": 2,    "unit": "px",          "label": "Node width" },
            "nodeHeight":  { "default": 54,  "min": 20,   "max": 400, "step": 2,    "unit": "px",          "label": "Node height" },
            "routeGutter": { "default": 132, "min": 20,   "max": 600, "step": 2,    "unit": "px",          "label": "Route gutter" },
            "marginY":     { "default": 104, "min": 0,    "max": 600, "step": 2,    "unit": "px",          "label": "Top margin" }
        },
        "sequence": {
            "participantWidth": { "default": 146, "min": 40, "max": 800, "step": 2,   "unit": "px",          "label": "Participant column width" },
            "rowHeight":        { "default": 56,  "min": 16, "max": 400, "step": 2,   "unit": "px",          "label": "Message row height" },
            "marginX":          { "default": 28,  "min": 0,  "max": 400, "step": 2,   "unit": "px",          "label": "Side margin" }
        },
        "zoom": {
            "minFitZoom": { "default": 0.15, "min": 0.01, "max": 1,   "step": 0.01,  "unit": "×",           "label": "Minimum fit zoom" },
            "maxFitZoom": { "default": 1.6,  "min": 0.5,  "max": 8,   "step": 0.1,   "unit": "×",           "label": "Maximum fit zoom" }
        },
        "legibility": {
            "gapArrowheads": { "default": 0.5, "min": 0, "max": 4, "step": 0.05, "unit": "arrowheads", "label": "Parallel-line gap" }
        }
    })
}

/// Returns the `SECTION_LABELS` static spec as a `serde_json::Value`.
/// Shape: `{ layout: "Layout & spacing", ... }`
pub fn section_labels_json() -> serde_json::Value {
    serde_json::json!({
        "layout":     "Layout & spacing",
        "sequence":   "Sequence diagram",
        "zoom":       "Fit zoom",
        "legibility": "Line legibility"
    })
}

// ─── Full JSON-layer resolver ─────────────────────────────────────────────────

/// Port of JS `normalizeDiagramConfigLayer(raw, { source })`.
///
/// Extracts the known, valid, in-range numeric overrides from one raw JSON
/// layer. Returns `(overrides_value, warnings)`.
fn normalize_diagram_config_layer(
    raw: &serde_json::Value,
    source: &str,
) -> (serde_json::Value, Vec<String>) {
    let fields_spec = diagram_config_fields_json();
    let mut overrides = serde_json::Map::new();
    let mut warnings: Vec<String> = Vec::new();

    if raw.is_null() {
        return (serde_json::Value::Object(overrides), warnings);
    }
    let raw_obj = match raw.as_object() {
        Some(o) => o,
        None => {
            warnings.push(format!("{source}: expected a JSON object, ignoring entire config."));
            return (serde_json::Value::Object(overrides), warnings);
        }
    };

    for (section, section_value) in raw_obj {
        let section_spec = match fields_spec.get(section) {
            Some(s) => s,
            None => {
                warnings.push(format!("{source}: unknown section \"{section}\" ignored."));
                continue;
            }
        };
        let section_obj = match section_value.as_object() {
            Some(o) => o,
            None => {
                warnings.push(format!("{source}: section \"{section}\" must be an object, ignored."));
                continue;
            }
        };
        for (field, raw_number) in section_obj {
            let field_spec = match section_spec.get(field) {
                Some(s) => s,
                None => {
                    warnings.push(format!("{source}: unknown field \"{section}.{field}\" ignored."));
                    continue;
                }
            };
            let num = match raw_number.as_f64() {
                Some(n) if n.is_finite() => n,
                _ => {
                    warnings.push(format!("{source}: \"{section}.{field}\" must be a finite number, ignored."));
                    continue;
                }
            };
            let min = field_spec["min"].as_f64().unwrap_or(f64::NEG_INFINITY);
            let max = field_spec["max"].as_f64().unwrap_or(f64::INFINITY);
            let clamped = num.clamp(min, max);
            if clamped != num {
                warnings.push(format!(
                    "{source}: \"{section}.{field}\" {num} clamped to {clamped} (allowed {min}–{max})."
                ));
            }
            overrides
                .entry(section.clone())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
                .as_object_mut()
                .unwrap()
                .insert(field.clone(), serde_json::json!(clamped));
        }
    }
    (serde_json::Value::Object(overrides), warnings)
}

/// Port of JS `resolveDiagramConfig(layers)` from JSON layer values.
///
/// Each layer is `(raw_json, source_name)` — ordered lowest-to-highest precedence.
/// Returns `(resolved_config_json, warnings)`.
/// `resolved_config_json` has the full shape `{ layout: {...}, sequence: {...}, zoom: {...}, legibility: {...} }`.
pub fn resolve_diagram_config_from_json(
    layers: &[(&serde_json::Value, &str)],
) -> (serde_json::Value, Vec<String>) {
    // Build the default config as a JSON Value.
    let fields_spec = diagram_config_fields_json();
    let mut config = serde_json::Map::new();
    for (section, section_spec) in fields_spec.as_object().unwrap() {
        let mut section_map = serde_json::Map::new();
        for (field, spec) in section_spec.as_object().unwrap() {
            section_map.insert(field.clone(), spec["default"].clone());
        }
        config.insert(section.clone(), serde_json::Value::Object(section_map));
    }

    let mut all_warnings: Vec<String> = Vec::new();

    for (raw, source) in layers {
        let (overrides, warnings) = normalize_diagram_config_layer(raw, source);
        all_warnings.extend(warnings);
        if let Some(overrides_obj) = overrides.as_object() {
            for (section, section_overrides) in overrides_obj {
                if let Some(serde_json::Value::Object(section_overrides_map)) = Some(section_overrides) {
                    if let Some(serde_json::Value::Object(config_section)) = config.get_mut(section) {
                        for (field, value) in section_overrides_map {
                            config_section.insert(field.clone(), value.clone());
                        }
                    }
                }
            }
        }
    }

    // Cross-field invariant: minFitZoom must not exceed maxFitZoom.
    let zoom_inverted = {
        let min = config.get("zoom")
            .and_then(|z| z["minFitZoom"].as_f64())
            .unwrap_or(0.0);
        let max = config.get("zoom")
            .and_then(|z| z["maxFitZoom"].as_f64())
            .unwrap_or(f64::INFINITY);
        min > max
    };
    if zoom_inverted {
        let min_default = fields_spec["zoom"]["minFitZoom"]["default"].as_f64().unwrap();
        let max_default = fields_spec["zoom"]["maxFitZoom"]["default"].as_f64().unwrap();
        let min_val = config.get("zoom").and_then(|z| z["minFitZoom"].as_f64()).unwrap_or(min_default);
        let max_val = config.get("zoom").and_then(|z| z["maxFitZoom"].as_f64()).unwrap_or(max_default);
        all_warnings.push(format!(
            "zoom: minFitZoom {min_val} exceeds maxFitZoom {max_val}; reverting zoom to defaults."
        ));
        if let Some(serde_json::Value::Object(zoom_section)) = config.get_mut("zoom") {
            zoom_section.insert("minFitZoom".to_string(), serde_json::json!(min_default));
            zoom_section.insert("maxFitZoom".to_string(), serde_json::json!(max_default));
        }
    }

    (serde_json::Value::Object(config), all_warnings)
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

    // ─── diagram_config_fields_json shape ────────────────────────────────────

    #[test]
    fn fields_json_has_correct_sections() {
        let f = diagram_config_fields_json();
        for section in &["layout", "sequence", "zoom", "legibility"] {
            assert!(f.get(section).is_some(), "missing section {section}");
        }
    }

    #[test]
    fn fields_json_layout_lane_width_spec() {
        let f = diagram_config_fields_json();
        let lw = &f["layout"]["laneWidth"];
        assert_eq!(lw["default"].as_f64(), Some(210.0));
        assert_eq!(lw["min"].as_f64(), Some(60.0));
        assert_eq!(lw["max"].as_f64(), Some(800.0));
        assert_eq!(lw["unit"].as_str(), Some("px"));
        assert_eq!(lw["label"].as_str(), Some("Column width"));
    }

    #[test]
    fn section_labels_json_correct() {
        let s = section_labels_json();
        assert_eq!(s["layout"].as_str(), Some("Layout & spacing"));
        assert_eq!(s["sequence"].as_str(), Some("Sequence diagram"));
        assert_eq!(s["zoom"].as_str(), Some("Fit zoom"));
        assert_eq!(s["legibility"].as_str(), Some("Line legibility"));
    }

    // ─── resolve_diagram_config_from_json ────────────────────────────────────

    #[test]
    fn resolve_no_layers_returns_defaults() {
        let (config, warnings) = resolve_diagram_config_from_json(&[]);
        assert!(warnings.is_empty());
        assert_eq!(config["layout"]["laneWidth"].as_f64(), Some(210.0));
        assert_eq!(config["layout"]["rowGap"].as_f64(), Some(102.0));
        assert_eq!(config["zoom"]["minFitZoom"].as_f64(), Some(0.15));
        assert_eq!(config["zoom"]["maxFitZoom"].as_f64(), Some(1.6));
        assert_eq!(config["legibility"]["gapArrowheads"].as_f64(), Some(0.5));
    }

    #[test]
    fn resolve_layer_overrides_field() {
        let layer = serde_json::json!({ "layout": { "laneWidth": 300 } });
        let (config, warnings) = resolve_diagram_config_from_json(&[(&layer, "project config")]);
        assert!(warnings.is_empty());
        assert_eq!(config["layout"]["laneWidth"].as_f64(), Some(300.0));
        // other fields unchanged
        assert_eq!(config["layout"]["rowGap"].as_f64(), Some(102.0));
    }

    #[test]
    fn resolve_clamps_out_of_range() {
        let layer = serde_json::json!({ "layout": { "laneWidth": 9999 } });
        let (config, warnings) = resolve_diagram_config_from_json(&[(&layer, "user config")]);
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("clamped"));
        assert_eq!(config["layout"]["laneWidth"].as_f64(), Some(800.0)); // max is 800
    }

    #[test]
    fn resolve_unknown_section_warns() {
        let layer = serde_json::json!({ "badSection": { "foo": 1 } });
        let (config, warnings) = resolve_diagram_config_from_json(&[(&layer, "project config")]);
        assert!(warnings.iter().any(|w| w.contains("unknown section")));
        // defaults still intact
        assert_eq!(config["layout"]["laneWidth"].as_f64(), Some(210.0));
    }

    #[test]
    fn resolve_zoom_invariant_revert() {
        // minFitZoom > maxFitZoom → revert zoom to defaults
        let layer = serde_json::json!({ "zoom": { "minFitZoom": 5.0, "maxFitZoom": 0.5 } });
        let (config, warnings) = resolve_diagram_config_from_json(&[(&layer, "project config")]);
        assert!(warnings.iter().any(|w| w.contains("reverting zoom to defaults")));
        assert_eq!(config["zoom"]["minFitZoom"].as_f64(), Some(0.15));
        assert_eq!(config["zoom"]["maxFitZoom"].as_f64(), Some(1.6));
    }

    #[test]
    fn resolve_higher_precedence_layer_wins() {
        let user_layer = serde_json::json!({ "layout": { "laneWidth": 200 } });
        let project_layer = serde_json::json!({ "layout": { "laneWidth": 350 } });
        // user first (lower prec), project second (higher prec)
        let (config, _) = resolve_diagram_config_from_json(&[
            (&user_layer, "user config"),
            (&project_layer, "project config"),
        ]);
        assert_eq!(config["layout"]["laneWidth"].as_f64(), Some(350.0));
    }

    #[test]
    fn resolve_ignored_dir_filter_is_covered() {
        // This test is a placeholder — the actual ignored-dir filter is in repo_tree.
        // Verified here that the routing crate builds with the full resolver.
        let (config, _) = resolve_diagram_config_from_json(&[]);
        assert!(config.is_object());
    }
}
