//! Pure Release Truth model: parse a release detail document and shape its
//! milestone/scope progression for the read-only Release Path projection.
//!
//! Faithful port of `viewer/src/presentation/releaseTruth.js` (the shaping
//! helpers) and the `ReleaseDetail` JSON shape (`viewer/src/domain/
//! architectureTypes.ts`). The detail file's shape is stable enough to type
//! directly here (the data layer holds it as raw JSON because V2 only needed the
//! summary fields); this slice deserializes the fields the Release Path reads.
//!
//! Leptos-free and native-testable. Status/posture COLOR is not decided here —
//! the panel maps the ported [`release_tone`] buckets onto the state/severity
//! token scale (never a `--c4-*` role hue), matching DESIGN.md.

use serde::Deserialize;

/// One release item across any scope bucket (required/planned/stretch/…).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub workstream_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseScope {
    #[serde(default)]
    pub required: Vec<ReleaseItem>,
    #[serde(default)]
    pub planned: Vec<ReleaseItem>,
    #[serde(default)]
    pub stretch: Vec<ReleaseItem>,
    #[serde(default)]
    pub deferred: Vec<ReleaseItem>,
    #[serde(default)]
    pub out_of_scope: Vec<ReleaseItem>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseMilestone {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub target_window: Option<String>,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseWorkstream {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub posture: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub progress: Option<i64>,
    #[serde(default)]
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseBlocker {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub item_ids: Vec<String>,
}

/// The release detail document, deserialized from `ReleaseDetail.raw`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDoc {
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
    #[serde(default)]
    pub target_date: Option<String>,
    #[serde(default)]
    pub target_window: Option<String>,
    #[serde(default)]
    pub scope: ReleaseScope,
    #[serde(default)]
    pub workstreams: Vec<ReleaseWorkstream>,
    #[serde(default)]
    pub blockers: Vec<ReleaseBlocker>,
    #[serde(default)]
    pub milestones: Vec<ReleaseMilestone>,
}

impl ReleaseDoc {
    /// Parse a raw release detail JSON value into the typed doc. Returns `None`
    /// if the shape is unexpected (FAIL LOUD: the panel surfaces a clear message
    /// rather than silently rendering an empty path).
    pub fn from_value(raw: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(raw.clone()).ok()
    }

    /// All scope items, in the JS `releaseItems` order
    /// (required → planned → stretch → deferred → outOfScope).
    pub fn items(&self) -> Vec<&ReleaseItem> {
        self.scope
            .required
            .iter()
            .chain(&self.scope.planned)
            .chain(&self.scope.stretch)
            .chain(&self.scope.deferred)
            .chain(&self.scope.out_of_scope)
            .collect()
    }

    /// `id -> scope-bucket label`, JS `releaseScopeByItemId`.
    pub fn scope_label(&self, item_id: &str) -> &'static str {
        if self.scope.required.iter().any(|i| i.id == item_id) {
            "required"
        } else if self.scope.planned.iter().any(|i| i.id == item_id) {
            "planned"
        } else if self.scope.stretch.iter().any(|i| i.id == item_id) {
            "stretch"
        } else if self.scope.deferred.iter().any(|i| i.id == item_id) {
            "deferred"
        } else if self.scope.out_of_scope.iter().any(|i| i.id == item_id) {
            "out of scope"
        } else {
            "scope"
        }
    }

    /// % of `required` items complete (JS `releaseProgress`), 0 when none.
    pub fn progress(&self) -> i64 {
        let req = &self.scope.required;
        if req.is_empty() {
            return 0;
        }
        let complete = req.iter().filter(|i| i.status.as_deref() == Some("complete")).count();
        ((complete as f64 / req.len() as f64) * 100.0).round() as i64
    }
}

/// One release item resolved for a Release Path line: the item, its scope label,
/// owning workstream name, the active blockers, and the coarse line state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathItem {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub priority: Option<String>,
    pub owner: Option<String>,
    pub scope: String,
    pub workstream_name: String,
    pub line_state: String,
    /// First active blocker's title, if any (JS `primaryBlocker.title`).
    pub blocked_by: Option<String>,
}

/// One milestone resolved against the doc's items + blockers, ready to render.
/// Fully owned (no borrows) so the synthetic "unlinked scope" milestone can be
/// constructed inline, matching the JS `ReleasePath`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneView {
    pub id: String,
    pub label: String,
    /// Milestone timing string (date → targetWindow → "No date"), JS `timing`.
    pub timing: String,
    pub items: Vec<PathItem>,
    /// Titles of the items that are blocked (the milestone "Blocked by:" line).
    pub blocked_by: Vec<String>,
    /// The effective milestone status (JS `releasePathMilestoneStatus`).
    pub status: String,
    /// Coarse milestone line state (JS `releaseLineState`).
    pub line_state: String,
    /// Path marker number: 0 for deferred/cut, else the milestone order.
    pub path_number: i64,
    pub completion_text: String,
    pub item_count: usize,
}

/// Build the ordered Release Path milestone views, appending an "unlinked scope"
/// synthetic milestone for items not referenced by any milestone (JS `ReleasePath`).
pub fn release_path(doc: &ReleaseDoc) -> Vec<MilestoneView> {
    let items = doc.items();
    let ws_name = |id: &Option<String>| -> String {
        match id {
            Some(wid) => doc
                .workstreams
                .iter()
                .find(|w| &w.id == wid)
                .map(|w| w.name.clone())
                .unwrap_or_else(|| "Unassigned".to_string()),
            None => "Unassigned".to_string(),
        }
    };

    let linked: std::collections::HashSet<&str> =
        doc.milestones.iter().flat_map(|m| m.item_ids.iter().map(String::as_str)).collect();
    let unlinked: Vec<&ReleaseItem> =
        items.iter().copied().filter(|i| !linked.contains(i.id.as_str())).collect();

    // Real milestones (cloned so the view is owned), plus the synthetic unlinked
    // milestone; then sort by order.
    let mut milestones: Vec<ReleaseMilestone> = doc.milestones.clone();
    if !unlinked.is_empty() {
        let max_order = doc.milestones.iter().map(|m| m.order).max().unwrap_or(0);
        milestones.push(ReleaseMilestone {
            id: "unlinked-release-scope".to_string(),
            label: "Other considered release scope".to_string(),
            status: Some("planned".to_string()),
            date: None,
            target_window: Some("Tracked outside explicit milestones".to_string()),
            order: max_order.max(0) + 1,
            item_ids: unlinked.iter().map(|i| i.id.clone()).collect(),
        });
    }
    milestones.sort_by_key(|m| m.order);

    milestones
        .iter()
        .map(|m| resolve_milestone(m, &items, doc, &ws_name))
        .collect()
}

/// Resolve one milestone's items/blocked-items/status/marker (JS milestone map).
fn resolve_milestone(
    m: &ReleaseMilestone,
    all_items: &[&ReleaseItem],
    doc: &ReleaseDoc,
    ws_name: &dyn Fn(&Option<String>) -> String,
) -> MilestoneView {
    let items: Vec<&ReleaseItem> = m
        .item_ids
        .iter()
        .filter_map(|id| all_items.iter().copied().find(|i| &i.id == id))
        .collect();
    let blocked: Vec<&ReleaseItem> = items
        .iter()
        .copied()
        .filter(|item| {
            item.status.as_deref() == Some("blocked")
                || !active_blockers_for_item(item, &doc.blockers).is_empty()
        })
        .collect();
    let status = milestone_status(m.status.as_deref(), &items, blocked.len());
    let path_number = if status == "deferred" || status == "cut" { 0 } else { m.order };
    let milestone_line_state = line_state(Some(&status), !blocked.is_empty()).to_string();

    let path_items = items
        .iter()
        .map(|item| {
            let active = active_blockers_for_item(item, &doc.blockers);
            let primary = active.first();
            let state = line_state(item.status.as_deref(), primary.is_some()).to_string();
            PathItem {
                id: item.id.clone(),
                title: item.title.clone(),
                summary: item.summary.clone(),
                status: item.status.clone(),
                kind: item.kind.clone(),
                priority: item.priority.clone(),
                owner: item.owner.clone(),
                scope: doc.scope_label(&item.id).to_string(),
                workstream_name: ws_name(&item.workstream_id),
                line_state: state,
                blocked_by: primary.map(|b| b.title.clone()),
            }
        })
        .collect();

    MilestoneView {
        id: m.id.clone(),
        label: m.label.clone(),
        timing: m
            .date
            .clone()
            .or_else(|| m.target_window.clone())
            .unwrap_or_else(|| "No date".to_string()),
        completion_text: completion_text(&items),
        blocked_by: blocked.iter().map(|i| i.title.clone()).collect(),
        item_count: items.len(),
        items: path_items,
        status,
        line_state: milestone_line_state,
        path_number,
    }
}

/// JS `releasePathMilestoneStatus`: all-complete → complete; any blocked →
/// blocked; else the authored status (defaults "planned").
pub fn milestone_status(status: Option<&str>, items: &[&ReleaseItem], blocked: usize) -> String {
    if !items.is_empty() && items.iter().all(|i| i.status.as_deref() == Some("complete")) {
        return "complete".to_string();
    }
    if blocked > 0 {
        return "blocked".to_string();
    }
    status.unwrap_or("planned").to_string()
}

/// JS `releasePathCompletionText`: "N/M complete".
pub fn completion_text(items: &[&ReleaseItem]) -> String {
    let complete = items.iter().filter(|i| i.status.as_deref() == Some("complete")).count();
    format!("{}/{} complete", complete, items.len())
}

/// JS `releaseLineState`: the coarse status word for a line.
pub fn line_state(status: Option<&str>, blocked: bool) -> &'static str {
    match status {
        Some("complete") => "Complete",
        Some("deferred") | Some("cut") => "Deferred",
        _ if blocked || status == Some("blocked") => "Blocked",
        _ => "Not Blocked",
    }
}

/// JS `releaseStatusCanShowBlockers`.
fn status_can_show_blockers(status: Option<&str>) -> bool {
    !matches!(status, Some("complete") | Some("deferred") | Some("cut"))
}

/// JS `activeReleaseBlockersForItem`: blockers that still apply to an item.
pub fn active_blockers_for_item<'a>(
    item: &ReleaseItem,
    blockers: &'a [ReleaseBlocker],
) -> Vec<&'a ReleaseBlocker> {
    if !status_can_show_blockers(item.status.as_deref()) {
        return Vec::new();
    }
    blockers
        .iter()
        .filter(|b| b.item_ids.iter().any(|id| id == &item.id))
        .filter(|b| status_can_show_blockers(b.status.as_deref()))
        .collect()
}

/// The five status/posture/severity tone buckets (JS `releaseTone`). The panel
/// maps these onto the state/severity token scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Healthy,
    Progressing,
    Blocked,
    Inactive,
    Neutral,
}

/// JS `releaseTone`: classify any release/posture/status/severity string.
pub fn release_tone(value: Option<&str>) -> Tone {
    match value {
        None => Tone::Neutral,
        Some(v) => match v {
            "complete" | "completed" | "shipped" | "on-track" | "low" => Tone::Healthy,
            "draft" | "planned" | "implementing" | "in-progress" | "release-candidate"
            | "stretch" | "medium" => Tone::Progressing,
            "blocked" | "at-risk" | "critical" | "high" => Tone::Blocked,
            "deferred" | "cut" => Tone::Inactive,
            _ => Tone::Neutral,
        },
    }
}

/// JS `releaseStatusLabels`: human label for an item/milestone status.
pub fn status_label(status: Option<&str>) -> &'static str {
    match status {
        Some("planned") => "Planned",
        Some("in-progress") => "In Progress",
        Some("blocked") => "Blocked",
        Some("complete") => "Complete",
        Some("deferred") => "Deferred",
        Some("stretch") => "Stretch",
        Some("cut") => "Cut",
        _ => "Planned",
    }
}

/// JS `formatReleaseDate`: trim an ISO datetime to its date.
pub fn format_release_date(value: Option<&str>) -> String {
    match value {
        Some(v) if v.contains('T') => v.split('T').next().unwrap_or(v).to_string(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> ReleaseDoc {
        let raw = serde_json::json!({
            "id": "v9-9-9",
            "version": "9.9.9",
            "name": "Test Release",
            "status": "implementing",
            "posture": "on-track",
            "targetWindow": "Q9",
            "scope": {
                "required": [
                    { "id": "a", "title": "A", "status": "complete", "kind": "feature" },
                    { "id": "b", "title": "B", "status": "in-progress", "kind": "feature", "workstreamId": "ws1" },
                    { "id": "c", "title": "C", "status": "planned", "kind": "feature" }
                ],
                "deferred": [
                    { "id": "d", "title": "D", "status": "deferred", "kind": "chore" }
                ]
            },
            "milestones": [
                { "id": "m2", "label": "Second", "status": "planned", "order": 2, "itemIds": ["c"] },
                { "id": "m1", "label": "First done", "status": "planned", "order": 1, "itemIds": ["a"] }
            ],
            "workstreams": [
                { "id": "ws1", "name": "WS One", "status": "in-progress", "posture": "on-track", "progress": 50, "itemIds": ["b"] }
            ],
            "blockers": []
        });
        ReleaseDoc::from_value(&raw).expect("doc parses")
    }

    #[test]
    fn items_are_in_scope_order_and_scope_labels_resolve() {
        let d = doc();
        assert_eq!(d.items().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c", "d"]);
        assert_eq!(d.scope_label("a"), "required");
        assert_eq!(d.scope_label("d"), "deferred");
        assert_eq!(d.scope_label("ghost"), "scope");
        // required = a(complete), b, c → 1/3 → 33%.
        assert_eq!(d.progress(), 33);
    }

    #[test]
    fn release_path_orders_milestones_and_computes_status() {
        let d = doc();
        let path = release_path(&d);
        // Sorted by order: m1 (order 1), m2 (order 2), then the synthetic
        // "unlinked-release-scope" milestone (item b and d are unlinked).
        assert_eq!(
            path.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            vec!["m1", "m2", "unlinked-release-scope"]
        );
        // m1 has only item a (complete) → all-complete → "complete".
        assert_eq!(path[0].status, "complete");
        assert_eq!(path[0].completion_text, "1/1 complete");
        assert_eq!(path[0].path_number, 1);
        // m2 has item c (planned), not blocked → authored "planned".
        assert_eq!(path[1].status, "planned");
        assert_eq!(path[1].completion_text, "0/1 complete");
        // The synthetic milestone collects unlinked items b and d.
        assert_eq!(
            path[2].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "d"]
        );
        // Item b's owning workstream resolves to its name.
        assert_eq!(path[2].items[0].workstream_name, "WS One");
    }

    #[test]
    fn tone_buckets_match_js() {
        assert_eq!(release_tone(Some("shipped")), Tone::Healthy);
        assert_eq!(release_tone(Some("in-progress")), Tone::Progressing);
        assert_eq!(release_tone(Some("blocked")), Tone::Blocked);
        assert_eq!(release_tone(Some("at-risk")), Tone::Blocked);
        assert_eq!(release_tone(Some("cut")), Tone::Inactive);
        assert_eq!(release_tone(None), Tone::Neutral);
        assert_eq!(release_tone(Some("whatever")), Tone::Neutral);
    }

    #[test]
    fn line_state_and_date_format() {
        assert_eq!(line_state(Some("complete"), false), "Complete");
        assert_eq!(line_state(Some("cut"), false), "Deferred");
        assert_eq!(line_state(Some("in-progress"), true), "Blocked");
        assert_eq!(line_state(Some("planned"), false), "Not Blocked");
        assert_eq!(format_release_date(Some("2026-06-15T12:00:00.000Z")), "2026-06-15");
        assert_eq!(format_release_date(Some("2026-06-15")), "2026-06-15");
        assert_eq!(format_release_date(None), "");
    }
}
