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

use serde::Deserialize;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub criticality: Option<String>,
    #[serde(default)]
    pub order: Option<i64>,
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

/// `/api/config` payload (`{ diagram, warnings, fields, sections }`). Kept as a
/// raw value here — the viewer surfaces the resolved diagram config and any
/// warnings without typing the full field/section spec, which the server owns.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigPayload {
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub diagram: serde_json::Value,
}
