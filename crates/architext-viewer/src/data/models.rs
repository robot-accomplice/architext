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

// ─── notes.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct NotesFile {
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// An element note — a user annotation attached to an architecture element
/// (node/flow/decision/risk/view/data-class), persisted in `notes.json`.
///
/// `Serialize` is derived (not just `Deserialize`) because the Notes editor
/// round-trips the FULL note back to `POST /api/notes`
/// (`{action:"update", note:<full note>}`); serializing with the same camelCase
/// field names keeps the upsert payload faithful to the on-disk shape and the
/// `additionalProperties:false` schema (no extra fields are emitted).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub target: NoteTarget,
    pub category: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// `note.target` — the element a note is attached to.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NoteTarget {
    pub kind: String,
    pub id: String,
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
    pub summary: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub target_release_id: Option<String>,
}

// ─── code-graph.json ───────────────────────────────────────────────────────

/// `code-graph.json` — the Magma-produced call graph (contract
/// `magma-code-graph/1`). Optional third-party enrichment: Architext validates
/// and renders it but never writes it.
///
/// TWO deliberate departures from every other model in this file:
///   1. NO `rename_all = "camelCase"` — the producer is a Go tool and the wire
///      format is snake_case, which already matches these Rust field names.
///   2. The four collections are `Option<Vec<_>>`, not `Vec<_>`: a REFUSAL
///      document (`computable: false`) sets them to JSON `null`, and
///      `#[serde(default)]` only covers a MISSING key, never an explicit null.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraph {
    pub contract_version: String,
    pub generator: String,
    pub language: String,
    pub module: String,
    pub sha: String,
    pub tree: String,
    pub fidelity: String,
    pub computable: bool,
    /// Whether producing this document EXECUTED the analysed repository's
    /// code (e.g. Rust's `build.rs` / proc macros) rather than only reading
    /// it. Optional: magma has not shipped it yet, and older artifacts won't
    /// carry it. A trust-boundary fact, not a fidelity nuance — drive any
    /// "this analysis ran your code" disclosure off THIS field, never off
    /// `language == "rust"` (magma's own rationale: that equivalence breaks
    /// once their deferred sandboxed mode lands).
    #[serde(default)]
    pub executed_target_code: Option<bool>,
    #[serde(default)]
    pub not_computable_reason: Option<String>,
    #[serde(default)]
    pub functions: Option<Vec<CodeGraphFunction>>,
    #[serde(default)]
    pub calls: Option<Vec<CodeGraphCall>>,
    #[serde(default)]
    pub modules: Option<Vec<CodeGraphModule>>,
    #[serde(default)]
    pub module_calls: Option<Vec<CodeGraphModuleCall>>,
}

/// One parameter. `name` is omitted for unnamed params; `type` is always present.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphSignatureParam {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub param_type: String,
}

/// One result. Results are never named.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphSignatureResult {
    #[serde(rename = "type")]
    pub result_type: String,
}

/// Always present on a function; both arrays always present, possibly empty.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphSignature {
    #[serde(default)]
    pub params: Vec<CodeGraphSignatureParam>,
    #[serde(default)]
    pub results: Vec<CodeGraphSignatureResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphFunction {
    pub id: String,
    pub symbol: String,
    pub pkg: String,
    pub file: String,
    pub line: u32,
    pub kind: String,
    pub exported: bool,
    pub test: bool,
    pub root: bool,
    pub generated: bool,
    pub reachable: bool,
    pub prod_reachable: bool,
    pub signature: CodeGraphSignature,
    /// First sentence of the doc comment; omitted when empty.
    #[serde(default)]
    pub doc: Option<String>,
    pub fan_in: u32,
    pub fan_out: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphCall {
    pub from: String,
    pub to: String,
    pub site_file: String,
    pub site_line: u32,
    /// `static` | `dynamic` (dynamic = RTA over-approximation).
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphModuleCounts {
    pub functions: u32,
    pub dead: u32,
    pub test_only: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphModule {
    pub id: String,
    pub pkg: String,
    #[serde(default)]
    pub function_ids: Vec<String>,
    pub counts: CodeGraphModuleCounts,
    /// Distinct INTER-module edge degree (module-graph degree) — not a sum of
    /// underlying call counts. Intra-module edges are excluded.
    pub fan_in: u32,
    pub fan_out: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodeGraphModuleCall {
    pub from: String,
    pub to: String,
    /// Underlying fine call edges collapsed into this module→module edge.
    pub count: u32,
    /// True when any underlying edge is a dynamic dispatch.
    pub has_dynamic: bool,
}

// ─── slop-ferret.json ───────────────────────────────────────────────────────

/// A slop-ferret sweep snapshot consumed by Architext. Optional third-party
/// enrichment, like `CodeGraph`.
#[derive(Debug, Clone, Deserialize)]
pub struct SlopFerret {
    pub schema: i64,
    #[serde(default)]
    pub origin: String,
    #[serde(default, rename = "root_commit")]
    pub root_commit: String,
    #[serde(default, rename = "identity_method")]
    pub identity_method: String,
    pub sha: String,
    pub date: String,
    #[serde(rename = "attested_repo")]
    pub attested_repo: String,
    #[serde(rename = "attested_plan")]
    pub attested_plan: String,
    pub denominator: i64,
    #[serde(default)]
    pub waived: i64,
    #[serde(default, rename = "worklist_size")]
    pub worklist_size: i64,
    #[serde(default, rename = "unmatched_size")]
    pub unmatched_size: i64,
    pub accounting: String,
    #[serde(default, rename = "vocab_provenance")]
    pub vocab_provenance: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub tier: String,
    #[serde(default, rename = "families_not_run")]
    pub families_not_run: Vec<String>,
    #[serde(default, rename = "checked_clean")]
    pub checked_clean: Vec<SlopFerretCheckedClean>,
    #[serde(default, rename = "near_misses")]
    pub near_misses: Vec<String>,
    #[serde(default, rename = "findings_verified")]
    pub findings_verified: Option<i64>,
    #[serde(default, rename = "findings_suspected")]
    pub findings_suspected: Option<i64>,
    #[serde(default, rename = "report_path")]
    pub report_path: String,
    #[serde(default)]
    pub findings: Vec<SlopFerretFinding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlopFerretCheckedClean {
    pub class: String,
    pub method: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlopFerretFinding {
    pub title: String,
    pub file: String,
    pub class: String,
    pub severity: String,
    pub status: String,
    #[serde(default)]
    pub claim: String,
    #[serde(default)]
    pub refutation: String,
    #[serde(default)]
    pub bar: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub remediation: String,
    #[serde(default)]
    pub occurrences: Option<i64>,
}

// ─── releases/index.json + detail files ──────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseIndex {
    #[serde(default)]
    pub current_release_id: Option<String>,
    pub releases: Vec<ReleaseSummary>,
}

/// Per-release roll-up counts (the `counts` object on each summary). Only the
/// two the trend chart plots are modeled; serde ignores the rest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCounts {
    #[serde(default)]
    pub features: i64,
    #[serde(default)]
    pub bug_fixes: i64,
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
    /// Feature/bug-fix roll-up, plotted by the release trend chart.
    #[serde(default)]
    pub counts: ReleaseCounts,
    /// Completion / target timestamps — the chart sorts + labels by these
    /// (releasedAt, else targetDate, else targetWindow), matching the React viewer.
    #[serde(default)]
    pub released_at: Option<String>,
    #[serde(default)]
    pub target_date: Option<String>,
    #[serde(default)]
    pub target_window: Option<String>,
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

// ─── /api/node-git ─────────────────────────────────────────────────────────

/// `/api/node-git?paths=` payload — a node's git "development window" derived
/// from its `sourcePaths`. `tracked` is false (and the rest absent) when the
/// paths are not in the serving repo (e.g. the sanitized review corpus).
#[derive(Debug, Clone, Deserialize)]
pub struct NodeGit {
    pub tracked: bool,
    #[serde(rename = "firstCommit", default)]
    pub first_commit: Option<String>,
    #[serde(rename = "lastCommit", default)]
    pub last_commit: Option<String>,
    #[serde(rename = "commitCount", default)]
    pub commit_count: Option<u64>,
    #[serde(default)]
    pub authors: Vec<String>,
}

// ─── /api/file ─────────────────────────────────────────────────────────────

/// `/api/file?path=` payload (`{ path, size, language, truncated, binary, html }`).
/// Fetched on demand by the Repo Tree file-preview pane when a file row is
/// clicked. `html` is server-rendered, inline-styled syntax-highlight HTML
/// (null for binary files); the viewer renders it directly with `inner_html`.
#[derive(Debug, Clone, Deserialize)]
pub struct FilePreviewPayload {
    pub path: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub binary: bool,
    #[serde(default)]
    pub html: Option<String>,
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

    #[test]
    fn code_graph_parses_a_computable_document() {
        // WHY: the wire format is snake_case (Go producer), unlike every other
        // Architext document — a stray rename_all would silently drop fields.
        let doc: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "magma/0.2.0",
            "language": "go",
            "module": "example.com/m",
            "sha": "abc1234",
            "tree": "clean",
            "fidelity": "rta",
            "computable": true,
            "functions": [{
                "id": "m-add", "symbol": "Add", "pkg": "m", "file": "main.go", "line": 3,
                "kind": "func", "exported": true, "test": false, "root": false,
                "generated": false, "reachable": true, "prod_reachable": true,
                "signature": {
                    "params": [{"name": "a", "type": "int"}, {"type": "int"}],
                    "results": [{"type": "int"}]
                },
                "doc": "Add returns the sum.", "fan_in": 1, "fan_out": 0
            }],
            "calls": [{"from": "m-main", "to": "m-add", "site_file": "main.go", "site_line": 7, "kind": "static"}],
            "modules": [{
                "id": "m", "pkg": "m", "function_ids": ["m-add"],
                "counts": {"functions": 1, "dead": 0, "test_only": 0},
                "fan_in": 0, "fan_out": 0
            }],
            "module_calls": [{"from": "m", "to": "m2", "count": 3, "has_dynamic": true}]
        })).expect("computable document must parse");

        assert!(doc.computable);
        let fns = doc.functions.expect("functions present");
        assert_eq!(fns[0].fan_in, 1);
        assert_eq!(fns[0].prod_reachable, true);
        assert_eq!(fns[0].signature.params.len(), 2);
        assert_eq!(fns[0].signature.params[0].name.as_deref(), Some("a"));
        // An unnamed param has no `name` key at all.
        assert_eq!(fns[0].signature.params[1].name, None);
        assert_eq!(fns[0].signature.params[1].param_type, "int");
        assert_eq!(fns[0].signature.results[0].result_type, "int");
        assert_eq!(doc.calls.unwrap()[0].site_line, 7);
        assert_eq!(doc.modules.unwrap()[0].counts.functions, 1);
        let mc = doc.module_calls.unwrap();
        assert_eq!(mc[0].count, 3);
        assert!(mc[0].has_dynamic);
    }

    #[test]
    fn code_graph_parses_a_refusal_with_null_arrays() {
        // WHY: a refusal sets the four collections to explicit JSON `null`.
        // #[serde(default)] only covers a MISSING key — a plain Vec<_> field
        // would fail here, so these MUST be Option<Vec<_>>. This test is the
        // guard against someone "simplifying" them back to Vec.
        let doc: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "magma/0.2.0",
            "language": "python",
            "module": "",
            "sha": "",
            "tree": "clean",
            "fidelity": "rta",
            "computable": false,
            "not_computable_reason": "unsupported language: python",
            "functions": null,
            "calls": null,
            "modules": null,
            "module_calls": null
        })).expect("refusal document must parse as VALID");

        assert!(!doc.computable);
        assert_eq!(doc.not_computable_reason.as_deref(), Some("unsupported language: python"));
        assert!(doc.functions.is_none());
        assert!(doc.calls.is_none());
        assert!(doc.modules.is_none());
        assert!(doc.module_calls.is_none());
    }

    #[test]
    fn code_graph_parses_executed_target_code_when_present_and_absent() {
        // WHY: `executed_target_code` reaching the viewer's serde model was the
        // gap this test guards — serde silently ignores unknown fields, so a
        // document could parse "successfully" while the field never reached the
        // UI. Present (magma's Rust artifacts) and absent (Go artifacts, and
        // every artifact predating the field) must both parse.
        let with_field: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1", "generator": "magma/0.3.0",
            "language": "rust", "module": "m", "sha": "a", "tree": "clean",
            "fidelity": "semantic", "computable": true, "executed_target_code": true,
            "functions": [], "calls": [], "modules": [], "module_calls": []
        }))
        .expect("document with executed_target_code must parse");
        assert_eq!(with_field.executed_target_code, Some(true));

        let without_field: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1", "generator": "magma/0.1.0",
            "language": "go", "module": "m", "sha": "a", "tree": "clean",
            "fidelity": "rta", "computable": true,
            "functions": [], "calls": [], "modules": [], "module_calls": []
        }))
        .expect("document without executed_target_code must still parse");
        assert_eq!(without_field.executed_target_code, None);
    }

    #[test]
    fn code_graph_omits_optional_doc_and_param_name() {
        // A function with no doc comment omits `doc` entirely; unnamed params
        // omit `name`. Neither may fail the parse.
        let doc: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "magma/0.2.0", "language": "go", "module": "m",
            "sha": "a", "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [{
                "id": "m-main", "symbol": "main", "pkg": "m", "file": "main.go", "line": 7,
                "kind": "func", "exported": false, "test": false, "root": true,
                "generated": false, "reachable": true, "prod_reachable": true,
                "signature": {"params": [], "results": []},
                "fan_in": 0, "fan_out": 1
            }],
            "calls": [], "modules": [], "module_calls": []
        })).expect("document without optional fields must parse");
        let fns = doc.functions.unwrap();
        assert!(fns[0].doc.is_none());
        assert!(fns[0].signature.params.is_empty());
    }
}
