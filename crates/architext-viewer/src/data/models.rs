//! Viewer-side serde models for the Architext data documents.
//!
//! These mirror the JSON shapes under `docs/architext/data/**`. They are
//! intentionally faithful but minimal: only fields the viewer reads are typed,
//! and unknown fields are ignored by serde's default behavior.
//!
//! Routing already owns the geometry-relevant `View`/`Flow`/`Lane`/`FlowStep`
//! types and the view-selection logic. The viewer models carry the richer
//! display fields (names, summaries, statuses) and provide cheap `to_routing`
//! adapters so the selection logic lives in exactly one place
//! (`architext_routing::plan_request::view_selection`).

use serde::{Deserialize, Serialize};

use architext_routing::plan_request::types::{
    Flow as RoutingFlow, FlowStep as RoutingFlowStep, Lane as RoutingLane, View as RoutingView,
};

// ─── manifest.json ─────────────────────────────────────────────────────────

/// `manifest.json` — names the project and maps logical doc names to paths.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: String,
    pub project: ManifestProject,
    #[serde(default)]
    pub default_view_id: Option<String>,
    /// logical name → relative path under the data dir.
    pub files: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestProject {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
}

// ─── nodes.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct NodesFile {
    pub nodes: Vec<Node>,
}

/// A node (component/actor/service/...). `node_type` carries the C4 role used
/// for the `--c4-*` chip token.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    /// Node ids this node structurally depends on. Drives the C4/deployment
    /// structural-relationship edges.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Repo paths (files or directory prefixes) this node owns. Drives Repo Tree
    /// ownership: a file whose path matches a node's `sourcePaths` prefix is
    /// owned by that node, and its row takes the node's `--c4-{type}` rail color.
    #[serde(rename = "sourcePaths", default)]
    pub source_paths: Vec<String>,
    /// Authored cross-references — flow/decision/risk/data-class ids this node
    /// declares it participates in. The Blast Radius reach unions these declared
    /// links with the derived ones (flow steps / `relatedNodes` back-references).
    /// Faithful to the JS `blastRadiusForNode` inputs.
    #[serde(rename = "relatedFlows", default)]
    pub related_flows: Vec<String>,
    #[serde(rename = "relatedDecisions", default)]
    pub related_decisions: Vec<String>,
    #[serde(rename = "knownRisks", default)]
    pub known_risks: Vec<String>,
    #[serde(rename = "dataHandled", default)]
    pub data_handled: Vec<String>,
}

// ─── views.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ViewsFile {
    pub views: Vec<View>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lane {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "nodeIds", default)]
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct View {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub view_type: String,
    #[serde(default)]
    pub summary: Option<String>,
    /// The node a decomposable C4 view scopes into — the drilldown anchor. A
    /// `c4-container` view with `scopeNodeId: "x"` is the child of node `x` in
    /// the parent `c4-context` view.
    #[serde(rename = "scopeNodeId", default)]
    pub scope_node_id: Option<String>,
    #[serde(default)]
    pub lanes: Vec<Lane>,
}

impl View {
    /// Total node count across all lanes (with duplicates, matching the raw
    /// authored membership the diagram renders).
    pub fn node_count(&self) -> usize {
        self.lanes.iter().map(|l| l.node_ids.len()).sum()
    }

    /// Adapt to the routing `View` used by view-selection.
    pub fn to_routing(&self) -> RoutingView {
        RoutingView {
            id: self.id.clone(),
            view_type: self.view_type.clone(),
            lanes: self
                .lanes
                .iter()
                .map(|l| RoutingLane { id: l.id.clone(), node_ids: l.node_ids.clone() })
                .collect(),
        }
    }
}

// ─── flows.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct FlowsFile {
    pub flows: Vec<Flow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowStep {
    pub id: String,
    pub from: String,
    pub to: String,
    pub action: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub return_of: Option<String>,
}

/// A sequence frame (`alt`/`loop`/`par`/`opt`/`transaction`) — a bordered box
/// spanning a contiguous range of the flow's steps in the SEQUENCE diagram. The
/// frame `type` labels the box (e.g. `loop: retry`); `step_ids` names the steps
/// it brackets. Only the SEQUENCE projection reads these.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceFrame {
    pub id: String,
    #[serde(rename = "type")]
    pub frame_type: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub step_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub steps: Vec<FlowStep>,
    /// SEQUENCE-mode frames bracketing step ranges. Absent for most flows.
    #[serde(default)]
    pub sequence_frames: Vec<SequenceFrame>,
}

impl Flow {
    /// Adapt to the routing `Flow` used by view-selection.
    pub fn to_routing(&self) -> RoutingFlow {
        RoutingFlow {
            id: self.id.clone(),
            steps: self
                .steps
                .iter()
                .map(|s| RoutingFlowStep {
                    id: s.id.clone(),
                    from: s.from.clone(),
                    to: s.to.clone(),
                    action: s.action.clone(),
                    summary: s.summary.clone(),
                    kind: s.kind.clone(),
                    outcome: s.outcome.clone(),
                    return_of: s.return_of.clone(),
                })
                .collect(),
        }
    }
}

// ─── data-classification.json ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct DataClassificationFile {
    pub classes: Vec<DataClass>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataClass {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sensitivity: Option<String>,
    #[serde(default)]
    pub handling: Option<String>,
}

// ─── decisions.json ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct DecisionsFile {
    pub decisions: Vec<Decision>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Decision {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    /// Node ids this decision references (the reverse-link source for Blast
    /// Radius: a node is reached by every decision whose `relatedNodes` names it).
    #[serde(rename = "relatedNodes", default)]
    pub related_nodes: Vec<String>,
}

// ─── risks.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct RisksFile {
    pub risks: Vec<Risk>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Risk {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// Node ids this risk references (the reverse-link source for Blast Radius).
    #[serde(rename = "relatedNodes", default)]
    pub related_nodes: Vec<String>,
}

// ─── glossary.json ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct GlossaryFile {
    pub terms: Vec<GlossaryTerm>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlossaryTerm {
    pub term: String,
    pub definition: String,
}

// ─── rules.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct RulesFile {
    pub rules: Vec<Rule>,
}

/// A project rule. `Serialize` is derived (not just `Deserialize`) because the
/// Rules editor round-trips the FULL rule back to `POST /api/rules`
/// (`{action:"update", rule:<full rule>}`); serializing with the same camelCase
/// field names keeps the upsert payload faithful to the on-disk shape.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criticality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i64>,
    /// Provenance (`maintainer`, `extracted`, ...). Display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Edit/delete protection flags.
    #[serde(default)]
    pub protection: RuleProtection,
}

/// `rule.protection` — whether the rule is edit/delete protected.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RuleProtection {
    #[serde(default)]
    pub edit: bool,
    #[serde(default)]
    pub delete: bool,
}

// ─── roadmap.json ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct RoadmapFile {
    #[serde(default)]
    pub items: Vec<RoadmapItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadmapItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub target_release_id: Option<String>,
}

// ─── releases/index.json + detail files ──────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseIndex {
    #[serde(default)]
    pub current_release_id: Option<String>,
    pub releases: Vec<ReleaseSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSummary {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub posture: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// Relative path of the detail file, under the `releases/` directory.
    #[serde(default)]
    pub file: Option<String>,
}

/// A release detail document (`releases/<id>.json`). Kept as a raw JSON value
/// because detail shapes vary across releases; V2 only needs the summary fields
/// for display, and rendering the full detail is a V3 concern.
#[derive(Debug, Clone)]
pub struct ReleaseDetail {
    pub id: String,
    pub raw: serde_json::Value,
}

// ─── /api/repo-tree ────────────────────────────────────────────────────────

/// `/api/repo-tree` payload (`{ source, files: [{path,size,mtime}] }`). Fetched
/// on demand by the Repo Tree surface (not part of the manifest-driven load).
#[derive(Debug, Clone, Deserialize)]
pub struct RepoTreePayload {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub files: Vec<RepoFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoFile {
    pub path: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub mtime: Option<i64>,
}

/// `/api/config` payload (`{ diagram, warnings, fields, sections }`).
///
/// `diagram` is the server-resolved config (defaults + the user/project layers);
/// `fields` is the `DIAGRAM_CONFIG_FIELDS` control spec (section → field →
/// `{default,min,max,step,unit,label[,options]}`) and `sections` is the
/// `SECTION_LABELS` map (section → human label). The config EDITOR renders one
/// control per field, grouped by section, driven entirely by this spec — never a
/// hardcoded field list.
///
/// The `POST /api/config` success body reuses this shape but omits `fields`/
/// `sections` (the server only re-sends `diagram` + `warnings`); both default to
/// empty so the response deserializes, and the editor keeps its existing spec
/// when merging the response back in.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigPayload {
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub diagram: serde_json::Value,
    /// The control spec grouped by section. Raw value (the server owns its
    /// shape); the editor reads it through [`ConfigPayload::sections_spec`].
    #[serde(default)]
    pub fields: serde_json::Value,
    /// Section id → display label.
    #[serde(default)]
    pub sections: serde_json::Value,
}

/// One configurable field's spec, parsed from the `fields` payload. Mirrors the
/// server's `{ default, min, max, step, unit, label }` (+ optional `options`).
/// `kind` is derived from the spec so the renderer picks a number input, a
/// select, or a toggle without a hardcoded per-field switch.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub default: serde_json::Value,
    pub unit: Option<String>,
}

/// The control kind a field renders as, inferred from its spec.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// A numeric input honoring `min`/`max`/`step`.
    Number { min: Option<f64>, max: Option<f64>, step: Option<f64> },
    /// A select over the spec's `options` (string values).
    Select { options: Vec<String> },
    /// A boolean toggle (spec `type: "bool"` or a boolean default).
    Bool,
}

/// One section of the config editor: its id, label, and ordered fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSection {
    pub id: String,
    pub label: String,
    pub fields: Vec<FieldSpec>,
}

impl ConfigPayload {
    /// Parse the `fields`/`sections` payload into ordered editor sections.
    ///
    /// Section order follows `sections` (the server's authored order, preserved
    /// because `serde_json` is built with `preserve_order`); within a section,
    /// field order follows the `fields[section]` object. Any section present in
    /// `fields` but absent from `sections` still renders, labeled by its id, so a
    /// new section can't silently drop out of the editor.
    pub fn sections_spec(&self) -> Vec<ConfigSection> {
        let fields = match self.fields.as_object() {
            Some(f) => f,
            None => return Vec::new(),
        };
        let labels = self.sections.as_object();

        // Authored section order from `sections`, then any field-only sections.
        let mut section_ids: Vec<String> = Vec::new();
        if let Some(labels) = labels {
            for id in labels.keys() {
                if fields.contains_key(id) {
                    section_ids.push(id.clone());
                }
            }
        }
        for id in fields.keys() {
            if !section_ids.contains(id) {
                section_ids.push(id.clone());
            }
        }

        section_ids
            .into_iter()
            .filter_map(|id| {
                let section_fields = fields.get(&id)?.as_object()?;
                let label = labels
                    .and_then(|l| l.get(&id))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&id)
                    .to_string();
                let specs = section_fields
                    .iter()
                    .map(|(key, spec)| parse_field_spec(key, spec))
                    .collect();
                Some(ConfigSection { id, label, fields: specs })
            })
            .collect()
    }
}

/// Parse one `fields[section][key]` spec object into a [`FieldSpec`]. The control
/// kind is inferred: explicit `options` → select, a boolean default or
/// `type: "bool"` → toggle, otherwise a number honoring min/max/step.
fn parse_field_spec(key: &str, spec: &serde_json::Value) -> FieldSpec {
    let label = spec
        .get("label")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(key)
        .to_string();
    let unit = spec
        .get("unit")
        .and_then(serde_json::Value::as_str)
        .filter(|u| !u.is_empty())
        .map(str::to_string);
    let default = spec.get("default").cloned().unwrap_or(serde_json::Value::Null);

    let kind = if let Some(options) = spec.get("options").and_then(serde_json::Value::as_array) {
        FieldKind::Select {
            options: options
                .iter()
                .map(|o| match o {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
        }
    } else if spec.get("type").and_then(serde_json::Value::as_str) == Some("bool")
        || default.is_boolean()
    {
        FieldKind::Bool
    } else {
        FieldKind::Number {
            min: spec.get("min").and_then(serde_json::Value::as_f64),
            max: spec.get("max").and_then(serde_json::Value::as_f64),
            step: spec.get("step").and_then(serde_json::Value::as_f64),
        }
    };

    FieldSpec { key: key.to_string(), label, kind, default, unit }
}

#[cfg(test)]
mod config_payload_tests {
    use super::*;
    use serde_json::json;

    /// The control spec mirrors the server's `/api/config` `fields`/`sections`.
    fn sample_payload() -> ConfigPayload {
        ConfigPayload {
            warnings: vec![],
            diagram: json!({ "layout": { "laneWidth": 300, "rowGap": 102 } }),
            fields: json!({
                "layout": {
                    "laneWidth": { "default": 210, "min": 60, "max": 800, "step": 2, "unit": "px", "label": "Column width" },
                    "rowGap": { "default": 102, "min": 20, "max": 600, "step": 2, "unit": "px", "label": "Row gap" }
                },
                "zoom": {
                    "minFitZoom": { "default": 0.15, "min": 0.01, "max": 1, "step": 0.01, "unit": "×", "label": "Minimum fit zoom" }
                }
            }),
            sections: json!({ "layout": "Layout & spacing", "zoom": "Fit zoom" }),
        }
    }

    #[test]
    fn sections_spec_follows_authored_order_and_labels() {
        let payload = sample_payload();
        let sections = payload.sections_spec();
        // Section order follows `sections` (preserve_order keeps layout, zoom).
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].id, "layout");
        assert_eq!(sections[0].label, "Layout & spacing");
        assert_eq!(sections[1].id, "zoom");
        assert_eq!(sections[1].label, "Fit zoom");
        // Field order within layout follows the fields object.
        let layout_keys: Vec<&str> = sections[0].fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(layout_keys, vec!["laneWidth", "rowGap"]);
    }

    #[test]
    fn field_spec_infers_number_kind_with_range() {
        let payload = sample_payload();
        let sections = payload.sections_spec();
        let lane = &sections[0].fields[0];
        assert_eq!(lane.key, "laneWidth");
        assert_eq!(lane.label, "Column width");
        assert_eq!(lane.unit.as_deref(), Some("px"));
        match &lane.kind {
            FieldKind::Number { min, max, step } => {
                assert_eq!(*min, Some(60.0));
                assert_eq!(*max, Some(800.0));
                assert_eq!(*step, Some(2.0));
            }
            other => panic!("expected Number, got {other:?}"),
        }
    }

    #[test]
    fn field_spec_infers_select_and_bool_kinds() {
        let payload = ConfigPayload {
            warnings: vec![],
            diagram: json!({}),
            fields: json!({
                "opt": {
                    "edgeStyle": { "default": "orthogonal", "options": ["orthogonal", "straight"], "label": "Edge style" },
                    "snap": { "default": false, "type": "bool", "label": "Snap" }
                }
            }),
            sections: json!({ "opt": "Options" }),
        };
        let sections = payload.sections_spec();
        let fields = &sections[0].fields;
        match &fields[0].kind {
            FieldKind::Select { options } => assert_eq!(options, &vec!["orthogonal".to_string(), "straight".to_string()]),
            other => panic!("expected Select, got {other:?}"),
        }
        assert_eq!(fields[1].kind, FieldKind::Bool);
    }

    #[test]
    fn sections_spec_empty_when_no_fields() {
        let payload = ConfigPayload::default();
        assert!(payload.sections_spec().is_empty());
    }

    #[test]
    fn field_only_section_renders_with_id_label() {
        // A section present in `fields` but missing from `sections` still renders.
        let payload = ConfigPayload {
            warnings: vec![],
            diagram: json!({}),
            fields: json!({ "extra": { "k": { "default": 1, "min": 0, "max": 10, "step": 1, "label": "K" } } }),
            sections: json!({}),
            ..Default::default()
        };
        let sections = payload.sections_spec();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].id, "extra");
        assert_eq!(sections[0].label, "extra");
    }
}
