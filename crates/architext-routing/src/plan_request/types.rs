//! Shared data types for the plan_request module.
//!
//! These are the in-memory representations of the JSON data that comes from
//! flows.json and views.json.

use serde::Deserialize;

/// One lane in a view (from views.json).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub id: String,
    pub node_ids: Vec<String>,
}

/// A view from views.json.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    #[serde(rename = "type")]
    pub view_type: String,
    pub lanes: Vec<Lane>,
}

/// One step in a flow.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowStep {
    pub id: String,
    pub from: String,
    pub to: String,
    pub action: String,
    #[serde(default)]
    pub summary: Option<String>,
    /// e.g. "decision", "return", "async", etc.
    pub kind: Option<String>,
    /// Present on outcome branches (the text label of the branch).
    pub outcome: Option<String>,
    /// For return steps: the id of the step this returns to.
    pub return_of: Option<String>,
}

/// A flow from flows.json.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub id: String,
    pub steps: Vec<FlowStep>,
}

/// Top-level flows.json shape.
#[derive(Debug, Deserialize)]
pub struct FlowsFile {
    pub flows: Vec<Flow>,
}

/// Top-level views.json shape.
#[derive(Debug, Deserialize)]
pub struct ViewsFile {
    pub views: Vec<View>,
}
