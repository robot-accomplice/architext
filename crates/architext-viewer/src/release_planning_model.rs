//! Pure Release Planning model: the helpers that the editor component drives.
//!
//! Faithful port of the pure logic in `viewer/src/presentation/ReleasePlanning.tsx`
//! and `viewer/src/presentation/releasePlanningModel.js`:
//!   - [`next_minor_version_from_releases`] — JS `nextMinorVersionFromReleases`.
//!   - [`sort_roadmap_items`] — JS `sortRoadmapItems` (section, then priority,
//!     then title).
//!   - [`planning_candidate_items`] — JS `planningCandidateItems` (items
//!     targeting the active release, deferred items, or items with no target).
//!   - [`editable_release_scope`] — JS `editableReleaseScope` (seed selection,
//!     per-item scope map, and ad-hoc items from an existing unreleased detail).
//!   - [`release_plan_proposal_payload`] — JS `releasePlanProposalPayload`.
//!   - [`release_plan_action_disabled`] — JS `releasePlanActionDisabled`.
//!
//! Leptos-free and native-testable. The serve contract is `POST
//! /api/release-plans` (see `crates/architext-serve/src/handlers/release_plans.rs`).

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::data::models::RoadmapItem;
use crate::release_truth::ReleaseDoc;

/// The five planning scope buckets, in the order the JS select lists them. The
/// stringly value is exactly the `ReleaseDetail.scope` key the serve handler
/// reads back (`outOfScope`, not `out-of-scope`).
pub const SCOPE_VALUES: &[&str] = &["required", "planned", "stretch", "deferred", "outOfScope"];

/// The default per-item scope when none is chosen (JS default `"planned"`).
pub const DEFAULT_SCOPE: &str = "planned";

/// The ad-hoc item kind options, in the JS select order.
pub const KIND_OPTIONS: &[(&str, &str)] = &[
    ("feature", "Feature"),
    ("bug-fix", "Bug fix"),
    ("documentation", "Documentation"),
    ("architecture", "Architecture"),
    ("test", "Test"),
    ("chore", "Chore"),
];

/// The priority options, severity-ordered (JS select order).
pub const PRIORITY_OPTIONS: &[&str] = &["critical", "high", "medium", "low"];

/// Human label for a scope value (JS `releasePlanningScopeLabels`).
pub fn scope_label(scope: &str) -> &'static str {
    match scope {
        "required" => "Required",
        "planned" => "Planned",
        "stretch" => "Stretch",
        "deferred" => "Deferred",
        "outOfScope" => "Out of scope",
        _ => "Planned",
    }
}

/// An ad-hoc planning item the maintainer typed in the editor (JS
/// `AdHocPlanningItem`). `persisted` marks an item seeded from an existing
/// detail (its `id` is sent back); a fresh item omits `id` in the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdHocPlanningItem {
    pub id: String,
    pub persisted: bool,
    pub title: String,
    pub summary: Option<String>,
    pub kind: String,
    pub priority: String,
    pub section: String,
    pub scope: String,
}

/// The seed for editing an existing unreleased plan (JS `editableReleaseScope`
/// return shape).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditableReleaseScope {
    pub selected_roadmap_ids: Vec<String>,
    /// `item id -> scope value`.
    pub item_scopes: BTreeMap<String, String>,
    pub ad_hoc_items: Vec<AdHocPlanningItem>,
}

/// JS `nextMinorVersionFromReleases`: the next `MAJOR.(MINOR+1).0` derived from
/// the highest semver in the release index. No releases → `0.1.0`.
pub fn next_minor_version_from_releases(versions: &[Option<String>]) -> String {
    let mut parsed: Vec<(u64, u64, u64)> = versions
        .iter()
        .filter_map(|v| v.as_deref())
        .filter_map(parse_semver)
        .collect();
    parsed.sort();
    let (major, minor, _patch) = parsed.last().copied().unwrap_or((0, 0, 0));
    format!("{}.{}.0", major, minor + 1)
}

/// Parse a strict `MAJOR.MINOR.PATCH` semver (JS regex `^(\d+)\.(\d+)\.(\d+)$`).
fn parse_semver(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// JS `sortRoadmapItems`: by section, then priority rank
/// (critical<high<medium<low, default medium), then title.
pub fn sort_roadmap_items(items: &[RoadmapItem]) -> Vec<RoadmapItem> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|left, right| {
        let section = section_of(left).cmp(section_of(right));
        if section != std::cmp::Ordering::Equal {
            return section;
        }
        let rank = priority_rank(left.priority.as_deref()).cmp(&priority_rank(right.priority.as_deref()));
        if rank != std::cmp::Ordering::Equal {
            return rank;
        }
        left.title.cmp(&right.title)
    });
    sorted
}

fn section_of(item: &RoadmapItem) -> &str {
    item.section.as_deref().unwrap_or("")
}

fn priority_rank(priority: Option<&str>) -> u8 {
    match priority {
        Some("critical") => 0,
        Some("high") => 1,
        Some("medium") | None => 2,
        Some("low") => 3,
        _ => 2,
    }
}

/// JS `planningCandidateItems`: items targeting the active release, OR deferred,
/// OR with no target release. `active_release_id` is the detail being edited (or
/// `None` when planning a fresh next release).
pub fn planning_candidate_items(items: &[RoadmapItem], active_release_id: Option<&str>) -> Vec<RoadmapItem> {
    items
        .iter()
        .filter(|item| {
            item.target_release_id.as_deref() == active_release_id
                || item.status.as_deref() == Some("deferred")
                || item.target_release_id.is_none()
        })
        .cloned()
        .collect()
}

/// JS `editableReleaseScope`: seed the editor from an existing unreleased detail.
/// Roadmap-sourced items pre-select; ad-hoc-sourced items become editable
/// ad-hoc rows; every non-cut item's scope is captured.
pub fn editable_release_scope(detail: &ReleaseDoc) -> EditableReleaseScope {
    let mut item_scopes: BTreeMap<String, String> = BTreeMap::new();
    for item in &detail.scope.required {
        item_scopes.insert(item.id.clone(), "required".to_string());
    }
    for item in &detail.scope.planned {
        item_scopes.insert(item.id.clone(), "planned".to_string());
    }
    for item in &detail.scope.stretch {
        item_scopes.insert(item.id.clone(), "stretch".to_string());
    }
    for item in &detail.scope.deferred {
        item_scopes.insert(item.id.clone(), "deferred".to_string());
    }
    for item in &detail.scope.out_of_scope {
        item_scopes.insert(item.id.clone(), "outOfScope".to_string());
    }

    let mut selected_roadmap_ids = Vec::new();
    let mut ad_hoc_items = Vec::new();
    for item in detail.items() {
        if item.status.as_deref() == Some("cut") {
            continue;
        }
        if item.source.as_deref() == Some("ad-hoc") {
            // Drop a summary identical to the title (JS:
            // `item.summary === item.title ? undefined : item.summary`).
            let summary = match item.summary.as_deref() {
                Some(s) if s != item.title => Some(s.to_string()),
                _ => None,
            };
            let section = item
                .workstream_id
                .as_deref()
                .and_then(|wid| detail.workstreams.iter().find(|w| w.id == wid))
                .map(|w| w.name.clone())
                .unwrap_or_else(|| "Ad hoc".to_string());
            ad_hoc_items.push(AdHocPlanningItem {
                id: item.id.clone(),
                persisted: true,
                title: item.title.clone(),
                summary,
                kind: item.kind.clone().unwrap_or_else(|| "feature".to_string()),
                priority: item.priority.clone().unwrap_or_else(|| "medium".to_string()),
                section,
                scope: item_scopes.get(&item.id).cloned().unwrap_or_else(|| DEFAULT_SCOPE.to_string()),
            });
        } else {
            selected_roadmap_ids.push(item.id.clone());
        }
    }

    EditableReleaseScope { selected_roadmap_ids, item_scopes, ad_hoc_items }
}

/// JS `releasePlanActionDisabled`: pending, blank version, or nothing selected.
pub fn release_plan_action_disabled(pending: bool, version: &str, selected_count: usize) -> bool {
    pending || version.trim().is_empty() || selected_count == 0
}

/// JS `releasePlanProposalPayload` (merged with the action-payload shape the
/// serve handler reads). `dry_run` is sent for legacy compat; `action` is the
/// authoritative selector.
pub fn release_plan_proposal_payload(
    dry_run: bool,
    action: &str,
    version: &str,
    theme: &str,
    selected_roadmap_ids: &[String],
    item_scopes: &BTreeMap<String, String>,
    ad_hoc_items: &[AdHocPlanningItem],
) -> Value {
    let scopes: serde_json::Map<String, Value> = item_scopes
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    let ad_hoc: Vec<Value> = ad_hoc_items
        .iter()
        .map(|item| {
            let mut obj = serde_json::Map::new();
            // A persisted (existing) item sends its id back; a fresh one omits it
            // so the server mints a new one.
            if item.persisted {
                obj.insert("id".to_string(), Value::String(item.id.clone()));
            }
            obj.insert("title".to_string(), Value::String(item.title.clone()));
            if let Some(summary) = &item.summary {
                obj.insert("summary".to_string(), Value::String(summary.clone()));
            }
            obj.insert("kind".to_string(), Value::String(item.kind.clone()));
            obj.insert("priority".to_string(), Value::String(item.priority.clone()));
            obj.insert("section".to_string(), Value::String(item.section.clone()));
            obj.insert("scope".to_string(), Value::String(item.scope.clone()));
            Value::Object(obj)
        })
        .collect();

    json!({
        "dryRun": dry_run,
        "action": action,
        "version": version,
        "theme": theme.trim(),
        "selectedRoadmapItemIds": selected_roadmap_ids,
        "itemScopes": Value::Object(scopes),
        "adHocItems": ad_hoc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roadmap_item(id: &str, section: &str, priority: Option<&str>, title: &str) -> RoadmapItem {
        RoadmapItem {
            id: id.to_string(),
            title: title.to_string(),
            summary: None,
            kind: Some("feature".to_string()),
            status: None,
            priority: priority.map(str::to_string),
            section: Some(section.to_string()),
            target_release_id: None,
        }
    }

    #[test]
    fn next_minor_picks_highest_semver_and_bumps_minor() {
        let versions = vec![
            Some("1.6.3".to_string()),
            Some("1.6.0".to_string()),
            Some("1.7.0".to_string()),
            Some("not-a-version".to_string()),
            None,
        ];
        assert_eq!(next_minor_version_from_releases(&versions), "1.8.0");
    }

    #[test]
    fn next_minor_no_releases_is_zero_one_zero() {
        assert_eq!(next_minor_version_from_releases(&[]), "0.1.0");
        assert_eq!(next_minor_version_from_releases(&[Some("garbage".to_string())]), "0.1.0");
    }

    #[test]
    fn sort_by_section_then_priority_then_title() {
        let items = vec![
            roadmap_item("c", "B", Some("low"), "Zeta"),
            roadmap_item("a", "A", Some("high"), "Beta"),
            roadmap_item("b", "A", Some("critical"), "Yota"),
            roadmap_item("d", "A", Some("high"), "Alpha"),
        ];
        let sorted = sort_roadmap_items(&items);
        let ids: Vec<&str> = sorted.iter().map(|i| i.id.as_str()).collect();
        // Section A first: critical(b), then high tie broken by title (Alpha=d, Beta=a),
        // then section B (c).
        assert_eq!(ids, vec!["b", "d", "a", "c"]);
    }

    #[test]
    fn candidates_match_target_deferred_or_untargeted() {
        let mut targeted = roadmap_item("t", "S", Some("high"), "Targeted");
        targeted.target_release_id = Some("v1-8-0".to_string());
        let mut other = roadmap_item("o", "S", Some("high"), "Other release");
        other.target_release_id = Some("v9-9-9".to_string());
        let mut deferred = roadmap_item("d", "S", Some("high"), "Deferred");
        deferred.status = Some("deferred".to_string());
        deferred.target_release_id = Some("v9-9-9".to_string());
        let untargeted = roadmap_item("u", "S", Some("high"), "Untargeted");

        let items = vec![targeted, other, deferred, untargeted];
        let candidates = planning_candidate_items(&items, Some("v1-8-0"));
        let ids: Vec<&str> = candidates.iter().map(|i| i.id.as_str()).collect();
        // "other" (targets a different release, not deferred, has a target) is excluded.
        assert_eq!(ids, vec!["t", "d", "u"]);
    }

    fn detail_with_ad_hoc() -> ReleaseDoc {
        let raw = json!({
            "id": "v1-8-0",
            "version": "1.8.0",
            "scope": {
                "required": [
                    { "id": "rm1", "title": "Roadmap one", "source": "roadmap", "status": "planned" }
                ],
                "planned": [
                    { "id": "ah1", "title": "Ad hoc one", "summary": "Different", "kind": "chore",
                      "priority": "low", "source": "ad-hoc", "status": "planned", "workstreamId": "ws1" }
                ],
                "stretch": [
                    { "id": "cut1", "title": "Cut item", "source": "roadmap", "status": "cut" }
                ]
            },
            "workstreams": [ { "id": "ws1", "name": "Tooling" } ],
            "blockers": [],
            "milestones": []
        });
        ReleaseDoc::from_value(&raw).expect("detail parses")
    }

    #[test]
    fn editable_scope_seeds_selection_scopes_and_ad_hoc() {
        let detail = detail_with_ad_hoc();
        let seed = editable_release_scope(&detail);
        // Roadmap-sourced, non-cut item is selected; the cut item is excluded.
        assert_eq!(seed.selected_roadmap_ids, vec!["rm1"]);
        // Per-item scope captured from the bucket the item lives in.
        assert_eq!(seed.item_scopes.get("rm1").map(String::as_str), Some("required"));
        assert_eq!(seed.item_scopes.get("ah1").map(String::as_str), Some("planned"));
        // The ad-hoc item is rebuilt with its workstream name as the section.
        assert_eq!(seed.ad_hoc_items.len(), 1);
        let ah = &seed.ad_hoc_items[0];
        assert_eq!(ah.id, "ah1");
        assert!(ah.persisted);
        assert_eq!(ah.section, "Tooling");
        assert_eq!(ah.scope, "planned");
        assert_eq!(ah.summary.as_deref(), Some("Different"));
    }

    #[test]
    fn editable_scope_drops_summary_equal_to_title() {
        let raw = json!({
            "id": "v1-8-0", "version": "1.8.0",
            "scope": { "planned": [
                { "id": "ah", "title": "Same", "summary": "Same", "kind": "feature", "source": "ad-hoc" }
            ] },
            "workstreams": [], "blockers": [], "milestones": []
        });
        let detail = ReleaseDoc::from_value(&raw).unwrap();
        let seed = editable_release_scope(&detail);
        assert_eq!(seed.ad_hoc_items[0].summary, None);
        // No workstream → default "Ad hoc" section.
        assert_eq!(seed.ad_hoc_items[0].section, "Ad hoc");
    }

    #[test]
    fn action_disabled_guards_pending_blank_version_empty_selection() {
        assert!(release_plan_action_disabled(true, "1.8.0", 3));
        assert!(release_plan_action_disabled(false, "  ", 3));
        assert!(release_plan_action_disabled(false, "1.8.0", 0));
        assert!(!release_plan_action_disabled(false, "1.8.0", 1));
    }

    #[test]
    fn payload_carries_contract_keys_and_trims_theme() {
        let mut scopes = BTreeMap::new();
        scopes.insert("rm1".to_string(), "required".to_string());
        let ad_hoc = vec![
            AdHocPlanningItem {
                id: "persisted".to_string(),
                persisted: true,
                title: "Kept".to_string(),
                summary: Some("S".to_string()),
                kind: "chore".to_string(),
                priority: "low".to_string(),
                section: "Sec".to_string(),
                scope: "planned".to_string(),
            },
            AdHocPlanningItem {
                id: "ad-hoc-12345".to_string(),
                persisted: false,
                title: "Fresh".to_string(),
                summary: None,
                kind: "feature".to_string(),
                priority: "medium".to_string(),
                section: "Sec".to_string(),
                scope: "stretch".to_string(),
            },
        ];
        let payload = release_plan_proposal_payload(
            true,
            "preview",
            "1.8.0",
            "  Theme  ",
            &["rm1".to_string()],
            &scopes,
            &ad_hoc,
        );
        assert_eq!(payload["action"], "preview");
        assert_eq!(payload["dryRun"], true);
        assert_eq!(payload["version"], "1.8.0");
        assert_eq!(payload["theme"], "Theme");
        assert_eq!(payload["selectedRoadmapItemIds"][0], "rm1");
        assert_eq!(payload["itemScopes"]["rm1"], "required");
        // Persisted ad-hoc item keeps its id; fresh one omits it.
        assert_eq!(payload["adHocItems"][0]["id"], "persisted");
        assert_eq!(payload["adHocItems"][0]["summary"], "S");
        assert!(payload["adHocItems"][1].get("id").is_none());
        assert!(payload["adHocItems"][1].get("summary").is_none());
        assert_eq!(payload["adHocItems"][1]["scope"], "stretch");
    }
}
