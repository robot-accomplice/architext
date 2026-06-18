//! Same-origin async fetch of the Architext data documents.
//!
//! The server (`architext-serve`) serves the data tree under `/data/<path>`
//! and the resolved diagram config under `/api/config`. The flow is:
//!   1. fetch `/data/manifest.json`
//!   2. fetch each manifest-referenced doc (`/data/<path>`)
//!   3. fetch the releases index + each referenced release detail file
//!   4. fetch `/api/config`
//!
//! All requests are same-origin relative URLs, so the viewer works wherever
//! the server mounts it. Any failed step surfaces a typed `FetchError` that the
//! UI renders as an explicit error surface (never a blank screen).

use gloo_net::http::Request;
use serde::de::DeserializeOwned;

use super::models::*;

/// A fully loaded architecture dataset. Missing-but-optional docs degrade to
/// empty collections; a missing *required* doc (manifest) is a hard error.
#[derive(Debug, Clone, Default)]
pub struct ArchitectureData {
    pub manifest: Option<Manifest>,
    pub nodes: Vec<Node>,
    pub views: Vec<View>,
    pub flows: Vec<Flow>,
    pub data_classes: Vec<DataClass>,
    pub decisions: Vec<Decision>,
    pub risks: Vec<Risk>,
    pub glossary: Vec<GlossaryTerm>,
    pub rules: Vec<Rule>,
    pub roadmap: Vec<RoadmapItem>,
    pub release_index: Option<ReleaseIndex>,
    pub release_details: Vec<ReleaseDetail>,
    pub config: Option<ConfigPayload>,
}

/// A failure during data load, with enough context to render a clear surface.
#[derive(Debug, Clone)]
pub struct FetchError {
    pub url: String,
    pub message: String,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.url, self.message)
    }
}

/// GET `url` and deserialize the JSON body into `T`.
async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, FetchError> {
    let resp = Request::get(url)
        .send()
        .await
        .map_err(|e| FetchError { url: url.to_string(), message: format!("request failed: {e}") })?;
    if !resp.ok() {
        return Err(FetchError {
            url: url.to_string(),
            message: format!("HTTP {} {}", resp.status(), resp.status_text()),
        });
    }
    resp.json::<T>()
        .await
        .map_err(|e| FetchError { url: url.to_string(), message: format!("invalid JSON: {e}") })
}

/// GET `url` as a raw JSON value (for release detail docs whose shape varies).
async fn get_value(url: &str) -> Result<serde_json::Value, FetchError> {
    get_json::<serde_json::Value>(url).await
}

/// Fetch the live repo file list from `/api/repo-tree`. Fetched on demand by the
/// Repo Tree surface (the file list is volatile, served `no-store`, and not part
/// of the once-loaded manifest dataset).
pub async fn fetch_repo_tree() -> Result<RepoTreePayload, FetchError> {
    get_json::<RepoTreePayload>("/api/repo-tree").await
}

/// Fetch the running CLI version from `/api/status` (`status.cliVersion`) for the
/// header eyebrow. Non-fatal — the version is display-only, so a failure (or an
/// older server without the field) degrades to `None` and the eyebrow omits it.
pub async fn fetch_cli_version() -> Option<String> {
    let status = get_value("/api/status").await.ok()?;
    status
        .get("status")
        .and_then(|s| s.get("cliVersion"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Resolve a manifest logical name to its `/data/<path>` URL, if present.
fn data_url(manifest: &Manifest, logical: &str) -> Option<String> {
    manifest.files.get(logical).map(|p| format!("/data/{p}"))
}

/// Load the full dataset over same-origin fetch. The manifest is required; every
/// other doc is optional and absent docs degrade to empty collections (the data
/// dir may legitimately omit, e.g., a roadmap).
pub async fn load_architecture_data() -> Result<ArchitectureData, FetchError> {
    let manifest: Manifest = get_json("/data/manifest.json").await?;

    let mut data = ArchitectureData { manifest: Some(manifest.clone()), ..Default::default() };

    if let Some(url) = data_url(&manifest, "nodes") {
        data.nodes = get_json::<NodesFile>(&url).await?.nodes;
    }
    if let Some(url) = data_url(&manifest, "views") {
        data.views = get_json::<ViewsFile>(&url).await?.views;
    }
    if let Some(url) = data_url(&manifest, "flows") {
        data.flows = get_json::<FlowsFile>(&url).await?.flows;
    }
    if let Some(url) = data_url(&manifest, "dataClassification") {
        data.data_classes = get_json::<DataClassificationFile>(&url).await?.classes;
    }
    if let Some(url) = data_url(&manifest, "decisions") {
        data.decisions = get_json::<DecisionsFile>(&url).await?.decisions;
    }
    if let Some(url) = data_url(&manifest, "risks") {
        data.risks = get_json::<RisksFile>(&url).await?.risks;
    }
    if let Some(url) = data_url(&manifest, "glossary") {
        data.glossary = get_json::<GlossaryFile>(&url).await?.terms;
    }
    if let Some(url) = data_url(&manifest, "rules") {
        data.rules = get_json::<RulesFile>(&url).await?.rules;
    }
    if let Some(url) = data_url(&manifest, "roadmap") {
        data.roadmap = get_json::<RoadmapFile>(&url).await?.items;
    }

    if let Some(index_path) = manifest.files.get("releases").cloned() {
        let index_url = format!("/data/{index_path}");
        let index: ReleaseIndex = get_json(&index_url).await?;
        // Detail files are relative to the releases index's directory.
        let base = index_path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
        for summary in &index.releases {
            if let Some(file) = &summary.file {
                let detail_path = if base.is_empty() {
                    file.clone()
                } else {
                    format!("{base}/{file}")
                };
                let detail_url = format!("/data/{detail_path}");
                let raw = get_value(&detail_url).await?;
                data.release_details.push(ReleaseDetail { id: summary.id.clone(), raw });
            }
        }
        data.release_index = Some(index);
    }

    // Config is same-origin but not under /data; a failure here is non-fatal so
    // the architecture still loads (config only affects diagram styling).
    data.config = get_json::<ConfigPayload>("/api/config").await.ok();

    Ok(data)
}
