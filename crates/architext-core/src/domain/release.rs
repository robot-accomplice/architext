//! Pure port of release-scopes.mjs, release-history.mjs, and release-planning.mjs.
//!
//! All functions operate on `serde_json::Value` (preserve_order enabled).
//! I/O-free — callers supply all document state.
//!
//! # DRY contract
//! `release_items`, `derive_release_counts`, `release_summary_from_detail` are
//! the single definitions; `crates/architext-core/src/validation/release.rs`
//! re-exports them from here instead of defining its own copies.

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

// ─────────────────────────────────────────────────────────────────────────────
// § release-scopes.mjs
// ─────────────────────────────────────────────────────────────────────────────

/// `releaseItems(detail)` — flat list of all scope items across every section.
///
/// Order: required → planned → stretch → deferred → outOfScope.
pub fn release_items(detail: &Value) -> Vec<&Value> {
    let scope = match detail.get("scope") {
        Some(s) => s,
        None => return vec![],
    };
    let mut items = Vec::new();
    for section in &["required", "planned", "stretch", "deferred", "outOfScope"] {
        if let Some(arr) = scope.get(section).and_then(Value::as_array) {
            items.extend(arr.iter());
        }
    }
    items
}

/// `releaseScopeEntries(scope)` — ordered `[(key, [items])]` pairs.
///
/// Returns a `Value::Array` of `[key, items]` pairs, mirroring JS `Array.entries`.
pub fn release_scope_entries(scope: &Value) -> Value {
    let sections = ["required", "planned", "stretch", "deferred", "outOfScope"];
    let pairs: Vec<Value> = sections
        .iter()
        .map(|&s| {
            let items = scope
                .get(s)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Value::Array(vec![Value::String(s.to_owned()), Value::Array(items)])
        })
        .collect();
    Value::Array(pairs)
}

// ─────────────────────────────────────────────────────────────────────────────
// § release-history.mjs
// ─────────────────────────────────────────────────────────────────────────────

/// `deriveReleaseCounts(detail)` — count items by kind/status.
pub fn derive_release_counts(detail: &Value) -> Value {
    let items = release_items(detail);
    let features = items.iter().filter(|i| i.get("kind").and_then(Value::as_str) == Some("feature")).count();
    let bug_fixes = items.iter().filter(|i| i.get("kind").and_then(Value::as_str) == Some("bug-fix")).count();
    let workstreams = detail.get("workstreams").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
    let blockers = detail.get("blockers").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
    let complete = items.iter().filter(|i| i.get("status").and_then(Value::as_str) == Some("complete")).count();
    let in_progress = items.iter().filter(|i| i.get("status").and_then(Value::as_str) == Some("in-progress")).count();
    let planned = items.iter().filter(|i| i.get("status").and_then(Value::as_str) == Some("planned")).count();
    let stretch = items.iter().filter(|i| i.get("status").and_then(Value::as_str) == Some("stretch")).count();
    json!({
        "features": features,
        "bugFixes": bug_fixes,
        "workstreams": workstreams,
        "blockers": blockers,
        "complete": complete,
        "inProgress": in_progress,
        "planned": planned,
        "stretch": stretch,
    })
}

/// `releaseSummaryFromDetail(detail, file)` — index summary from detail doc.
///
/// Conditional spreads: targetDate/targetWindow/releasedAt only appear when truthy.
pub fn release_summary_from_detail(detail: &Value, file: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("id".into(), str_or_null(detail, "id"));
    obj.insert("version".into(), str_or_null(detail, "version"));
    obj.insert("name".into(), str_or_null(detail, "name"));
    obj.insert("status".into(), str_or_null(detail, "status"));
    obj.insert("posture".into(), str_or_null(detail, "posture"));
    // Conditional spreads — only include if truthy (non-empty string)
    if let Some(td) = detail.get("targetDate").and_then(Value::as_str) {
        if !td.is_empty() {
            obj.insert("targetDate".into(), Value::String(td.to_owned()));
        }
    }
    if let Some(tw) = detail.get("targetWindow").and_then(Value::as_str) {
        if !tw.is_empty() {
            obj.insert("targetWindow".into(), Value::String(tw.to_owned()));
        }
    }
    if let Some(ra) = detail.get("releasedAt").and_then(Value::as_str) {
        if !ra.is_empty() {
            obj.insert("releasedAt".into(), Value::String(ra.to_owned()));
        }
    }
    obj.insert("lastUpdated".into(), str_or_null(detail, "lastUpdated"));
    obj.insert("summary".into(), str_or_null(detail, "summary"));
    obj.insert("counts".into(), derive_release_counts(detail));
    obj.insert("file".into(), Value::String(file.to_owned()));
    Value::Object(obj)
}

/// `generatedReleaseIndex(existingIndex, detailEntries)`.
///
/// `detailEntries` is a `Value::Array` of `{"detail": ..., "file": "..."}` objects.
pub fn generated_release_index(existing_index: &Value, detail_entries: &Value) -> Value {
    let entries = match detail_entries.as_array() {
        Some(a) => a,
        None => return json!({ "currentReleaseId": "", "releases": [] }),
    };

    let mut summaries: Vec<Value> = entries
        .iter()
        .filter_map(|e| {
            let detail = e.get("detail")?;
            let file = e.get("file")?.as_str()?;
            Some(release_summary_from_detail(detail, file))
        })
        .collect();

    // Sort by releaseSortKey (localeCompare equivalent — plain Rust string sort is
    // sufficient here: dates/versions are ASCII-comparable).
    summaries.sort_by(|a, b| {
        release_sort_key(a).cmp(&release_sort_key(b))
    });

    let summary_ids: std::collections::HashSet<&str> = summaries
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_str))
        .collect();

    let current_release_id = if let Some(existing_id) = existing_index
        .get("currentReleaseId")
        .and_then(Value::as_str)
    {
        if summary_ids.contains(existing_id) {
            existing_id.to_owned()
        } else {
            summaries.last().and_then(|s| s.get("id").and_then(Value::as_str)).unwrap_or("").to_owned()
        }
    } else {
        summaries.last().and_then(|s| s.get("id").and_then(Value::as_str)).unwrap_or("").to_owned()
    };

    json!({
        "currentReleaseId": current_release_id,
        "releases": summaries,
    })
}

/// `releaseIndexGenerationChanges(existingIndex, generatedIndex)`.
pub fn release_index_generation_changes(existing_index: &Value, generated_index: &Value) -> Value {
    let mut changes: Vec<Value> = Vec::new();

    // Null existing → single "generate" change
    if existing_index.is_null() {
        changes.push(Value::String("generate Release Truth history index from release detail files".to_owned()));
        return Value::Array(changes);
    }

    // currentReleaseId mismatch
    let existing_current = existing_index.get("currentReleaseId").and_then(Value::as_str).unwrap_or("");
    let generated_current = generated_index.get("currentReleaseId").and_then(Value::as_str).unwrap_or("");
    if existing_current != generated_current {
        changes.push(Value::String("refresh Release Truth currentReleaseId from available detail files".to_owned()));
    }

    let existing_releases = existing_index.get("releases").and_then(Value::as_array).cloned().unwrap_or_default();
    let generated_releases = generated_index.get("releases").and_then(Value::as_array).cloned().unwrap_or_default();

    let existing_by_id: std::collections::HashMap<&str, &Value> = existing_releases
        .iter()
        .filter_map(|r| r.get("id").and_then(Value::as_str).map(|id| (id, r)))
        .collect();
    let generated_by_id: std::collections::HashMap<&str, &Value> = generated_releases
        .iter()
        .filter_map(|r| r.get("id").and_then(Value::as_str).map(|id| (id, r)))
        .collect();

    for release in &generated_releases {
        let id = match release.get("id").and_then(Value::as_str) { Some(s) => s, None => continue };
        match existing_by_id.get(id) {
            None => {
                changes.push(Value::String(format!("add {id} to Release Truth history")));
            }
            Some(existing) => {
                if !same_summary(existing, release) {
                    changes.push(Value::String(format!("refresh generated Release Truth history for {id}")));
                }
            }
        }
    }

    for release in &existing_releases {
        let id = match release.get("id").and_then(Value::as_str) { Some(s) => s, None => continue };
        if !generated_by_id.contains_key(id) {
            changes.push(Value::String(format!("remove stale {id} from Release Truth history")));
        }
    }

    Value::Array(changes)
}

// ─────────────────────────────────────────────────────────────────────────────
// § release-planning.mjs
// ─────────────────────────────────────────────────────────────────────────────

/// `nextMinorVersion(releaseIndex)`.
pub fn next_minor_version(release_index: &Value) -> Value {
    let releases = match release_index.get("releases").and_then(Value::as_array) {
        Some(a) => a,
        None => return Value::String("0.1.0".to_owned()),
    };

    let mut versions: Vec<[u64; 3]> = releases
        .iter()
        .filter_map(|r| r.get("version").and_then(Value::as_str))
        .filter_map(parse_semver)
        .collect();

    // Sort by [major, minor, patch]
    versions.sort();

    let latest = versions.last().copied().unwrap_or([0, 0, 0]);
    Value::String(format!("{}.{}.0", latest[0], latest[1] + 1))
}

/// `buildReleasePlan({...})` — assemble a new release detail from roadmap + ad-hoc items.
pub fn build_release_plan(input: &Value) -> Result<Value, String> {
    let now = match input.get("now").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_owned(),
        _ => return Err("buildReleasePlan requires an explicit now timestamp.".to_owned()),
    };

    let release_index = &input["releaseIndex"];
    let roadmap_items = input.get("roadmapItems").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
    let selected_ids_arr = input.get("selectedRoadmapItemIds").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
    let item_scopes_val = input.get("itemScopes").cloned().unwrap_or_else(|| json!({}));
    let ad_hoc_items = input.get("adHocItems").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
    let project_name = input.get("projectName").and_then(Value::as_str).unwrap_or("");
    let theme = input.get("theme").and_then(Value::as_str);

    let version = if let Some(v) = input.get("version").and_then(Value::as_str) {
        v.to_owned()
    } else {
        match next_minor_version(release_index) {
            Value::String(s) => s,
            _ => "0.1.0".to_owned(),
        }
    };

    let id = release_id_for_version(&version);

    // Validate selectedRoadmapItemIds
    let roadmap_id_set: std::collections::HashSet<&str> = roadmap_items
        .iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str))
        .collect();
    for sel in selected_ids_arr {
        if let Some(s) = sel.as_str() {
            if !roadmap_id_set.contains(s) {
                return Err(format!("selectedRoadmapItemIds references unknown id \"{s}\""));
            }
        }
    }

    let selected_ids: std::collections::HashSet<&str> = selected_ids_arr
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // Build context
    let mut used_item_ids: std::collections::HashSet<String> = roadmap_items
        .iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str).map(|s| s.to_owned()))
        .collect();

    // workstreamsBySection: IndexMap to preserve insertion order
    let mut workstreams_by_section: IndexMap<String, Value> = IndexMap::new();
    let mut workstream_used_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut scope_required: Vec<Value> = Vec::new();
    let mut scope_planned: Vec<Value> = Vec::new();
    let mut scope_stretch: Vec<Value> = Vec::new();
    let mut scope_deferred: Vec<Value> = Vec::new();
    let mut scope_out_of_scope: Vec<Value> = Vec::new();

    // Helper closure to get-or-create workstream for a section name
    macro_rules! workstream_for_section {
        ($section:expr) => {{
            let name = if $section.is_empty() { "Ad hoc".to_owned() } else { $section.to_owned() };
            if !workstreams_by_section.contains_key(&name) {
                let ws_id = unique_id(&name, &mut workstream_used_ids);
                let ws = json!({
                    "id": ws_id,
                    "name": name,
                    "owner": "maintainer",
                    "status": "planned",
                    "posture": "on-track",
                    "summary": format!("{name} release scope."),
                    "progress": 0,
                    "itemIds": [],
                    "evidence": []
                });
                workstreams_by_section.insert(name.clone(), ws);
            }
            name
        }};
    }

    // assignRoadmapItems
    for item in roadmap_items {
        let item_id = match item.get("id").and_then(Value::as_str) { Some(s) => s, None => continue };
        if !selected_ids.contains(item_id) { continue; }

        // Check committed
        if let Some(target) = item.get("targetReleaseId").and_then(Value::as_str) {
            if !target.is_empty() && target != id {
                let item_status = item.get("status").and_then(Value::as_str).unwrap_or("");
                if item_status != "deferred" {
                    let title = item.get("title").and_then(Value::as_str).unwrap_or("");
                    return Err(format!(
                        "Roadmap item \"{title}\" is already committed to {target}. Defer it before moving it to {id}."
                    ));
                }
            }
        }

        let section = item.get("section").and_then(Value::as_str).unwrap_or("");
        let ws_name = workstream_for_section!(section);
        let ws = workstreams_by_section.get_mut(&ws_name).unwrap();
        let ws_id = ws["id"].as_str().unwrap().to_owned();

        let item_scope_str = item_scopes_val.get(item_id).and_then(Value::as_str).unwrap_or("planned");
        let release_scope = normalized_scope(item_scope_str);
        let release_item = release_item_from_roadmap(item, &ws_id, &now, release_scope);

        ws.as_object_mut().unwrap()
            .get_mut("itemIds").unwrap()
            .as_array_mut().unwrap()
            .push(Value::String(item_id.to_owned()));

        scope_push(&mut scope_required, &mut scope_planned, &mut scope_stretch, &mut scope_deferred, &mut scope_out_of_scope, release_scope, release_item);
    }

    // assignAdHocItems
    for item in ad_hoc_items {
        let section = item.get("section").and_then(Value::as_str).unwrap_or("Ad hoc");
        let ws_name = workstream_for_section!(section);
        let ws = workstreams_by_section.get_mut(&ws_name).unwrap();
        let ws_id = ws["id"].as_str().unwrap().to_owned();

        let release_item = release_item_from_ad_hoc(item, &mut used_item_ids, &ws_id, &now)?;
        let item_id = release_item["id"].as_str().unwrap().to_owned();
        let item_scope_str = item.get("scope").and_then(Value::as_str).unwrap_or("planned");
        let release_scope = normalized_scope(item_scope_str);

        ws.as_object_mut().unwrap()
            .get_mut("itemIds").unwrap()
            .as_array_mut().unwrap()
            .push(Value::String(item_id));

        scope_push(&mut scope_required, &mut scope_planned, &mut scope_stretch, &mut scope_deferred, &mut scope_out_of_scope, release_scope, release_item);
    }

    let scope = json!({
        "required": scope_required,
        "planned": scope_planned,
        "stretch": scope_stretch,
        "deferred": scope_deferred,
        "outOfScope": scope_out_of_scope,
    });

    let workstreams: Vec<Value> = workstreams_by_section.into_values().collect();

    // assemble release detail
    let title = if let Some(t) = theme {
        format!("{project_name} {version} {t}")
    } else {
        format!("{project_name} {version}")
    };
    let summary_str = if let Some(t) = theme {
        format!("{t} release plan.")
    } else {
        format!("{project_name} {version} release plan.")
    };

    // All scope items for milestone
    let tmp_detail = json!({ "scope": &scope });
    let all_items = release_items(&tmp_detail);
    let all_item_ids: Vec<Value> = all_items
        .iter()
        .filter_map(|i| i.get("id"))
        .cloned()
        .collect();

    let detail = json!({
        "id": id,
        "version": version,
        "name": title,
        "status": "planned",
        "posture": "on-track",
        "summary": summary_str,
        "targetWindow": "Next release",
        "lastUpdated": now,
        "updateSource": "Release Planning",
        "scope": scope,
        "workstreams": workstreams,
        "blockers": [],
        "milestones": [{
            "id": "planned-scope",
            "label": "Planned scope selected",
            "status": "planned",
            "targetWindow": "Release planning",
            "order": 1,
            "itemIds": all_item_ids
        }],
        "dependencies": [],
        "evidence": []
    });

    Ok(detail)
}

/// `mergeExistingReleasePlan(existingDetail, proposedDetail)`.
pub fn merge_existing_release_plan(existing_detail: &Value, proposed_detail: &Value) -> Value {
    // If no existing or id mismatch → return proposed as-is
    if existing_detail.is_null() {
        return proposed_detail.clone();
    }
    let existing_id = existing_detail.get("id").and_then(Value::as_str).unwrap_or("");
    let proposed_id = proposed_detail.get("id").and_then(Value::as_str).unwrap_or("");
    if existing_id != proposed_id {
        return proposed_detail.clone();
    }

    let existing_items_by_id: std::collections::HashMap<&str, &Value> = release_items(existing_detail)
        .into_iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str).map(|id| (id, i)))
        .collect();

    let existing_workstreams_by_id: std::collections::HashMap<&str, &Value> = existing_detail
        .get("workstreams")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter_map(|w| w.get("id").and_then(Value::as_str).map(|id| (id, w)))
        .collect();

    // Merge scope
    let proposed_scope = &proposed_detail["scope"];
    let sections = ["required", "planned", "stretch", "deferred", "outOfScope"];
    let mut merged_scope = Map::new();
    for s in &sections {
        let items = proposed_scope.get(s).and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
        let merged: Vec<Value> = items.iter().map(|item| {
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
            merged_release_item(existing_items_by_id.get(item_id).copied(), item)
        }).collect();
        merged_scope.insert((*s).to_owned(), Value::Array(merged));
    }

    // Merge workstreams
    let proposed_workstreams = proposed_detail.get("workstreams").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
    let merged_workstreams: Vec<Value> = proposed_workstreams.iter().map(|ws| {
        let ws_id = ws.get("id").and_then(Value::as_str).unwrap_or("");
        merged_workstream(existing_workstreams_by_id.get(ws_id).copied(), ws)
    }).collect();

    // status: if existing is "draft", use proposed status; else keep existing status
    let existing_status = existing_detail.get("status").and_then(Value::as_str).unwrap_or("");
    let status = if existing_status == "draft" {
        proposed_detail.get("status").and_then(Value::as_str).unwrap_or("planned").to_owned()
    } else {
        existing_status.to_owned()
    };

    // posture, summary: from existing
    let posture = existing_detail.get("posture").cloned().unwrap_or_else(|| proposed_detail["posture"].clone());
    let summary = {
        let es = existing_detail.get("summary").and_then(Value::as_str).unwrap_or("");
        if !es.is_empty() { Value::String(es.to_owned()) } else { proposed_detail["summary"].clone() }
    };

    // targetDate/targetWindow/releasedAt: existing ?? proposed.
    // Fixtures use explicit null when absent so both JS and Rust produce null.
    let target_date = existing_detail.get("targetDate").cloned()
        .unwrap_or_else(|| proposed_detail.get("targetDate").cloned().unwrap_or(Value::Null));
    let target_window = existing_detail.get("targetWindow").cloned()
        .unwrap_or_else(|| proposed_detail.get("targetWindow").cloned().unwrap_or(Value::Null));
    let released_at = existing_detail.get("releasedAt").cloned()
        .unwrap_or_else(|| proposed_detail.get("releasedAt").cloned().unwrap_or(Value::Null));

    // evidence: existing if non-empty, else proposed
    let evidence = {
        let ee = existing_detail.get("evidence").and_then(Value::as_array);
        if ee.map(|a| !a.is_empty()).unwrap_or(false) {
            existing_detail["evidence"].clone()
        } else {
            proposed_detail.get("evidence").cloned().unwrap_or_else(|| Value::Array(vec![]))
        }
    };

    // Build output: spread proposedDetail, then override specific keys
    let mut out = proposed_detail.clone();
    let out_obj = out.as_object_mut().unwrap();
    out_obj.insert("status".into(), Value::String(status));
    out_obj.insert("posture".into(), posture);
    out_obj.insert("summary".into(), summary);
    out_obj.insert("targetDate".into(), target_date);
    out_obj.insert("targetWindow".into(), target_window);
    out_obj.insert("releasedAt".into(), released_at);
    out_obj.insert("updateSource".into(), proposed_detail.get("updateSource").cloned().unwrap_or(Value::Null));
    out_obj.insert("scope".into(), Value::Object(merged_scope));
    out_obj.insert("workstreams".into(), Value::Array(merged_workstreams));
    out_obj.insert("blockers".into(), existing_detail.get("blockers").cloned().unwrap_or_else(|| Value::Array(vec![])));
    out_obj.insert("dependencies".into(), existing_detail.get("dependencies").cloned().unwrap_or_else(|| Value::Array(vec![])));
    out_obj.insert("evidence".into(), evidence);
    out
}

/// `releasePlanChanges({releaseIndex, roadmap, releaseDetail, file?, mode?})`.
pub fn release_plan_changes(input: &Value) -> Value {
    let release_index = &input["releaseIndex"];
    let roadmap = &input["roadmap"];
    let release_detail = &input["releaseDetail"];
    let release_id = release_detail.get("id").and_then(Value::as_str).unwrap_or("");
    let mode = input.get("mode").and_then(Value::as_str).unwrap_or("approve");
    let file = input.get("file").and_then(Value::as_str)
        .map(|s| s.to_owned())
        .unwrap_or_else(|| release_file_for_id(release_id));

    let releases = release_index.get("releases").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
    let existing_release = releases.iter().find(|r| r.get("id").and_then(Value::as_str) == Some(release_id));

    let roadmap_items_by_id: std::collections::HashMap<&str, &Value> = roadmap
        .get("items")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str).map(|id| (id, i)))
        .collect();

    let roadmap_changes: Vec<Value> = if mode == "draft" {
        vec![]
    } else {
        release_items(release_detail)
            .into_iter()
            .filter(|item| item.get("status").and_then(Value::as_str) != Some("cut"))
            .map(|item| {
                let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
                roadmap_change_for_item(item, roadmap_items_by_id.get(item_id).copied(), release_detail)
            })
            .collect()
    };

    let add_count = roadmap_changes.iter().filter(|c| c.get("action").and_then(Value::as_str) == Some("add")).count();
    let retarget_count = roadmap_changes.iter().filter(|c| c.get("action").and_then(Value::as_str) == Some("retarget")).count();
    let unchanged_count = roadmap_changes.iter().filter(|c| c.get("action").and_then(Value::as_str) == Some("unchanged")).count();

    let release_count = if existing_release.is_some() {
        releases.len()
    } else {
        releases.len() + 1
    };

    let current_release_id = if mode == "draft" {
        release_index.get("currentReleaseId").cloned().unwrap_or(Value::Null)
    } else {
        Value::String(release_id.to_owned())
    };

    json!({
        "releaseFile": {
            "action": if existing_release.is_some() { "replace" } else { "create" },
            "file": file,
            "id": release_id,
            "name": release_detail.get("name").and_then(Value::as_str).unwrap_or("")
        },
        "releaseIndex": {
            "action": if existing_release.is_some() { "replace-summary" } else { "add-summary" },
            "currentReleaseId": current_release_id,
            "releaseCount": release_count
        },
        "roadmap": {
            "add": add_count,
            "retarget": retarget_count,
            "unchanged": unchanged_count,
            "changes": roadmap_changes
        }
    })
}

/// `saveReleasePlanDraft({releaseIndex, roadmap, releaseDetail, file?})`.
pub fn save_release_plan_draft(input: &Value) -> Value {
    let release_index = &input["releaseIndex"];
    let roadmap = &input["roadmap"];
    let release_detail = &input["releaseDetail"];
    let file = input.get("file").and_then(Value::as_str)
        .map(|s| s.to_owned())
        .unwrap_or_else(|| {
            let id = release_detail.get("id").and_then(Value::as_str).unwrap_or("");
            release_file_for_id(id)
        });

    let mut draft_detail = release_detail.clone();
    draft_detail.as_object_mut().unwrap().insert("status".into(), Value::String("draft".to_owned()));

    let release_summary = release_summary_from_detail(&draft_detail, &file);
    let draft_id = draft_detail.get("id").and_then(Value::as_str).unwrap_or("");

    let mut releases: Vec<Value> = release_index
        .get("releases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.get("id").and_then(Value::as_str) != Some(draft_id))
        .collect();
    releases.push(release_summary);

    let mut new_index = release_index.clone();
    new_index.as_object_mut().unwrap().insert("releases".into(), Value::Array(releases));

    let changes_input = json!({
        "releaseIndex": release_index,
        "roadmap": roadmap,
        "releaseDetail": draft_detail,
        "file": file,
        "mode": "draft"
    });

    json!({
        "releaseIndex": new_index,
        "roadmap": roadmap,
        "releaseFile": {
            "file": file,
            "detail": draft_detail
        },
        "changes": release_plan_changes(&changes_input)
    })
}

/// `approveReleasePlan({releaseIndex, roadmap, releaseDetail, file?})`.
pub fn approve_release_plan(input: &Value) -> Value {
    let release_index = &input["releaseIndex"];
    let roadmap = &input["roadmap"];
    let release_detail = &input["releaseDetail"];
    let file = input.get("file").and_then(Value::as_str)
        .map(|s| s.to_owned())
        .unwrap_or_else(|| {
            let id = release_detail.get("id").and_then(Value::as_str).unwrap_or("");
            release_file_for_id(id)
        });

    // status: "draft" → "planned", else keep
    let status = match release_detail.get("status").and_then(Value::as_str) {
        Some("draft") => "planned".to_owned(),
        Some(s) => s.to_owned(),
        None => "planned".to_owned(),
    };
    let mut approved_detail = release_detail.clone();
    approved_detail.as_object_mut().unwrap().insert("status".into(), Value::String(status));

    let approved_id = approved_detail.get("id").and_then(Value::as_str).unwrap_or("");
    let release_summary = release_summary_from_detail(&approved_detail, &file);

    // Update releaseIndex
    let mut releases: Vec<Value> = release_index
        .get("releases")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.get("id").and_then(Value::as_str) != Some(approved_id))
        .collect();
    releases.push(release_summary);

    let mut new_index = release_index.clone();
    {
        let idx_obj = new_index.as_object_mut().unwrap();
        idx_obj.insert("currentReleaseId".into(), Value::String(approved_id.to_owned()));
        idx_obj.insert("releases".into(), Value::Array(releases));
    }

    // Update roadmap: IndexMap to preserve insertion order of existing items + new appends
    let roadmap_items = roadmap.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    // Use IndexMap to preserve existing item order, appending new ones at end
    let mut roadmap_items_by_id: IndexMap<String, Value> = roadmap_items
        .into_iter()
        .filter_map(|item| {
            let id = item.get("id").and_then(Value::as_str)?.to_owned();
            Some((id, item))
        })
        .collect();

    for item in release_items(&approved_detail) {
        if item.get("status").and_then(Value::as_str) == Some("cut") { continue; }
        let item_id = match item.get("id").and_then(Value::as_str) { Some(s) => s, None => continue };
        if let Some(existing) = roadmap_items_by_id.get_mut(item_id) {
            // Update targetReleaseId
            existing.as_object_mut().unwrap().insert("targetReleaseId".into(), Value::String(approved_id.to_owned()));
            // dateAdded backfill
            let item_date = item.get("dateAdded").and_then(Value::as_str);
            let existing_date = existing.get("dateAdded").and_then(Value::as_str);
            if let (Some(da), None) = (item_date, existing_date) {
                if !da.is_empty() {
                    existing.as_object_mut().unwrap().insert("dateAdded".into(), Value::String(da.to_owned()));
                }
            }
        } else {
            // Add new roadmap item
            let new_item = roadmap_item_from_release_item(item, &approved_detail);
            roadmap_items_by_id.insert(item_id.to_owned(), new_item);
        }
    }

    let new_roadmap_items: Vec<Value> = roadmap_items_by_id.into_values().collect();
    let mut new_roadmap = roadmap.clone();
    new_roadmap.as_object_mut().unwrap().insert("items".into(), Value::Array(new_roadmap_items));

    let changes_input = json!({
        "releaseIndex": release_index,
        "roadmap": roadmap,
        "releaseDetail": approved_detail,
        "file": file
    });

    json!({
        "releaseIndex": new_index,
        "roadmap": new_roadmap,
        "releaseFile": {
            "file": file,
            "detail": approved_detail
        },
        "changes": release_plan_changes(&changes_input)
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

/// `slug(value)` — JS slugification chain.
fn slug(value: &str) -> String {
    // .toLowerCase()
    let lower = value.to_lowercase();
    // .replace(/[^a-z0-9]+/g, "-")
    let mut s = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else {
            s.push('-');
        }
    }
    // Collapse runs: equivalent to replace(/[^a-z0-9]+/g, "-") already produces single dashes
    // because we push one '-' per non-alnum char. But we need to collapse consecutive '-'.
    // Actually each non-alnum char pushes its own '-', so "hello world!" → "hello-world-".
    // We need to collapse consecutive '-'.
    let s = collapse_dashes(&s);
    // .replace(/^-+|-+$/g, "") — trim leading/trailing dashes
    let s = s.trim_matches('-').to_owned();
    // .replace(/-{2,}/g, "-") — collapse remaining consecutive dashes (already done above)
    s
}

fn collapse_dashes(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_dash = false;
    for c in s.chars() {
        if c == '-' {
            if !last_was_dash {
                result.push('-');
            }
            last_was_dash = true;
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }
    result
}

/// `uniqueId(base, usedIds)` — generate a unique id, appending -2, -3, ... on collision.
fn unique_id(base: &str, used_ids: &mut std::collections::HashSet<String>) -> String {
    let normalized = {
        let s = slug(base);
        if s.is_empty() { "release-item".to_owned() } else { s }
    };
    if !used_ids.contains(&normalized) {
        used_ids.insert(normalized.clone());
        return normalized;
    }
    let mut index = 2u32;
    loop {
        let candidate = format!("{normalized}-{index}");
        if !used_ids.contains(&candidate) {
            used_ids.insert(candidate.clone());
            return candidate;
        }
        index += 1;
    }
}

fn release_id_for_version(version: &str) -> String {
    format!("v{}", version.replace('.', "-"))
}

fn release_file_for_id(id: &str) -> String {
    format!("{id}.json")
}

fn normalized_scope(value: &str) -> &'static str {
    match value {
        "out-of-scope" => "outOfScope",
        "required" => "required",
        "planned" => "planned",
        "stretch" => "stretch",
        "deferred" => "deferred",
        "outOfScope" => "outOfScope",
        _ => "planned",
    }
}

fn release_status_for_scope(scope: &str) -> &'static str {
    match scope {
        "deferred" => "deferred",
        "outOfScope" => "cut",
        _ => "planned",
    }
}

fn scope_push(
    required: &mut Vec<Value>,
    planned: &mut Vec<Value>,
    stretch: &mut Vec<Value>,
    deferred: &mut Vec<Value>,
    out_of_scope: &mut Vec<Value>,
    scope: &str,
    item: Value,
) {
    match scope {
        "required" => required.push(item),
        "planned" => planned.push(item),
        "stretch" => stretch.push(item),
        "deferred" => deferred.push(item),
        "outOfScope" => out_of_scope.push(item),
        _ => planned.push(item),
    }
}

fn release_item_from_roadmap(item: &Value, workstream_id: &str, date_added: &str, scope: &str) -> Value {
    let mut obj = Map::new();
    obj.insert("id".into(), item.get("id").cloned().unwrap_or(Value::Null));
    obj.insert("title".into(), item.get("title").cloned().unwrap_or(Value::Null));
    obj.insert("kind".into(), item.get("kind").cloned().unwrap_or(Value::Null));
    obj.insert("status".into(), Value::String(release_status_for_scope(scope).to_owned()));
    obj.insert("summary".into(), item.get("summary").cloned().unwrap_or(Value::Null));
    if let Some(p) = item.get("priority") {
        if !p.is_null() && p.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
            obj.insert("priority".into(), p.clone());
        }
    }
    obj.insert("source".into(), Value::String("roadmap".to_owned()));
    obj.insert("dateAdded".into(), Value::String(date_added.to_owned()));
    obj.insert("workstreamId".into(), Value::String(workstream_id.to_owned()));
    let depends_on = item.get("dependsOn").and_then(Value::as_array).cloned().unwrap_or_default();
    obj.insert("dependsOn".into(), Value::Array(depends_on));
    let evidence = item.get("evidence").and_then(Value::as_array).cloned().unwrap_or_default();
    obj.insert("evidence".into(), Value::Array(evidence));
    Value::Object(obj)
}

fn release_item_from_ad_hoc(
    item: &Value,
    used_item_ids: &mut std::collections::HashSet<String>,
    workstream_id: &str,
    date_added: &str,
) -> Result<Value, String> {
    let title = item.get("title").and_then(Value::as_str).unwrap_or("");
    let scope_str = item.get("scope").and_then(Value::as_str).unwrap_or("planned");
    let scope = normalized_scope(scope_str);

    if item.get("kind").and_then(Value::as_str).map(|s| s.is_empty()).unwrap_or(true) {
        return Err(format!("Ad hoc release item \"{title}\" must include a kind."));
    }
    let priority = item.get("priority").and_then(Value::as_str);
    if priority.map(|s| s.is_empty()).unwrap_or(true) {
        return Err(format!("Ad hoc release item \"{title}\" must include a priority."));
    }

    let id = if let Some(explicit_id) = item.get("id").and_then(Value::as_str) {
        explicit_id.to_owned()
    } else {
        unique_id(title, used_item_ids)
    };

    // summary?.trim() || item.title
    let summary_raw = item.get("summary").and_then(Value::as_str).unwrap_or("");
    let summary = {
        let trimmed = summary_raw.trim();
        if trimmed.is_empty() { title } else { trimmed }
    };

    let mut obj = Map::new();
    obj.insert("id".into(), Value::String(id));
    obj.insert("title".into(), Value::String(title.to_owned()));
    obj.insert("kind".into(), item.get("kind").cloned().unwrap_or(Value::Null));
    obj.insert("status".into(), Value::String(release_status_for_scope(scope).to_owned()));
    obj.insert("summary".into(), Value::String(summary.to_owned()));
    obj.insert("priority".into(), item.get("priority").cloned().unwrap_or(Value::Null));
    obj.insert("source".into(), Value::String("ad-hoc".to_owned()));
    obj.insert("dateAdded".into(), Value::String(date_added.to_owned()));
    obj.insert("workstreamId".into(), Value::String(workstream_id.to_owned()));
    let depends_on = item.get("dependsOn").and_then(Value::as_array).cloned().unwrap_or_default();
    obj.insert("dependsOn".into(), Value::Array(depends_on));
    let evidence = item.get("evidence").and_then(Value::as_array).cloned().unwrap_or_default();
    obj.insert("evidence".into(), Value::Array(evidence));
    Ok(Value::Object(obj))
}

fn merged_release_item(existing: Option<&Value>, proposed: &Value) -> Value {
    let existing = match existing {
        Some(e) => e,
        None => return proposed.clone(),
    };

    let existing_status = existing.get("status").and_then(Value::as_str).unwrap_or("");
    let proposed_status = proposed.get("status").and_then(Value::as_str).unwrap_or("");
    let preserve = matches!(existing_status, "complete" | "in-progress" | "blocked")
        && !matches!(proposed_status, "deferred" | "cut");

    let mut out = proposed.clone();
    let obj = out.as_object_mut().unwrap();

    // source: existing.source ?? proposed.source
    if let Some(s) = existing.get("source") {
        if !s.is_null() {
            obj.insert("source".into(), s.clone());
        }
    }
    // dateAdded: existing.dateAdded ?? proposed.dateAdded
    if let Some(da) = existing.get("dateAdded") {
        if !da.is_null() {
            obj.insert("dateAdded".into(), da.clone());
        }
    }
    // status preservation
    if preserve {
        obj.insert("status".into(), Value::String(existing_status.to_owned()));
    }
    // owner (only if truthy in existing)
    if let Some(owner) = existing.get("owner") {
        if owner.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
            obj.insert("owner".into(), owner.clone());
        }
    }
    // rationale
    if let Some(r) = existing.get("rationale") {
        if r.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
            obj.insert("rationale".into(), r.clone());
        }
    }
    // decisionSource
    if let Some(ds) = existing.get("decisionSource") {
        if ds.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
            obj.insert("decisionSource".into(), ds.clone());
        }
    }
    // dependsOn: existing if non-empty
    if let Some(dep) = existing.get("dependsOn").and_then(Value::as_array) {
        if !dep.is_empty() {
            obj.insert("dependsOn".into(), Value::Array(dep.clone()));
        }
    }
    // evidence: existing if non-empty
    if let Some(ev) = existing.get("evidence").and_then(Value::as_array) {
        if !ev.is_empty() {
            obj.insert("evidence".into(), Value::Array(ev.clone()));
        }
    }
    // deferredToReleaseId
    if let Some(dtri) = existing.get("deferredToReleaseId") {
        if !dtri.is_null() {
            obj.insert("deferredToReleaseId".into(), dtri.clone());
        }
    }
    // deferredToVersion
    if let Some(dtv) = existing.get("deferredToVersion") {
        if !dtv.is_null() {
            obj.insert("deferredToVersion".into(), dtv.clone());
        }
    }
    out
}

fn merged_workstream(existing: Option<&Value>, proposed: &Value) -> Value {
    let existing = match existing {
        Some(e) => e,
        None => return proposed.clone(),
    };
    let mut out = proposed.clone();
    let obj = out.as_object_mut().unwrap();
    // status, posture: always from existing
    obj.insert("status".into(), existing.get("status").cloned().unwrap_or_else(|| proposed["status"].clone()));
    obj.insert("posture".into(), existing.get("posture").cloned().unwrap_or_else(|| proposed["posture"].clone()));
    // summary: existing if truthy, else proposed
    let es = existing.get("summary").and_then(Value::as_str).unwrap_or("");
    if !es.is_empty() {
        obj.insert("summary".into(), Value::String(es.to_owned()));
    }
    // progress: existing ?? proposed
    if let Some(p) = existing.get("progress") {
        if !p.is_null() {
            obj.insert("progress".into(), p.clone());
        }
    }
    // evidence: existing if non-empty
    if let Some(ev) = existing.get("evidence").and_then(Value::as_array) {
        if !ev.is_empty() {
            obj.insert("evidence".into(), Value::Array(ev.clone()));
        }
    }
    out
}

fn roadmap_status_from_release_item(item: &Value) -> &'static str {
    match item.get("status").and_then(Value::as_str) {
        Some("complete") => "complete",
        Some("in-progress") => "in-progress",
        Some("deferred") => "deferred",
        Some("cut") => "cut",
        _ => "planned",
    }
}

fn release_item_section(item: &Value, release_detail: &Value) -> String {
    let workstream_id = item.get("workstreamId").and_then(Value::as_str).unwrap_or("");
    let workstreams = release_detail.get("workstreams").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]);
    for ws in workstreams {
        if ws.get("id").and_then(Value::as_str) == Some(workstream_id) {
            if let Some(name) = ws.get("name").and_then(Value::as_str) {
                return name.to_owned();
            }
        }
    }
    "Ad hoc".to_owned()
}

fn roadmap_item_from_release_item(item: &Value, release_detail: &Value) -> Value {
    let release_id = release_detail.get("id").and_then(Value::as_str).unwrap_or("");
    let mut obj = Map::new();
    obj.insert("id".into(), item.get("id").cloned().unwrap_or(Value::Null));
    obj.insert("title".into(), item.get("title").cloned().unwrap_or(Value::Null));
    obj.insert("summary".into(), item.get("summary").cloned().unwrap_or(Value::Null));
    obj.insert("kind".into(), item.get("kind").cloned().unwrap_or(Value::Null));
    obj.insert("status".into(), Value::String(roadmap_status_from_release_item(item).to_owned()));
    if let Some(p) = item.get("priority") {
        if !p.is_null() && p.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
            obj.insert("priority".into(), p.clone());
        }
    }
    obj.insert("section".into(), Value::String(release_item_section(item, release_detail)));
    obj.insert("targetReleaseId".into(), Value::String(release_id.to_owned()));
    if let Some(da) = item.get("dateAdded").and_then(Value::as_str) {
        if !da.is_empty() {
            obj.insert("dateAdded".into(), Value::String(da.to_owned()));
        }
    }
    if let Some(ev) = item.get("evidence").and_then(Value::as_array) {
        if !ev.is_empty() {
            obj.insert("evidence".into(), Value::Array(ev.clone()));
        }
    }
    Value::Object(obj)
}

fn roadmap_change_for_item(item: &Value, existing: Option<&Value>, release_detail: &Value) -> Value {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
    let item_title = item.get("title").and_then(Value::as_str).unwrap_or("");
    let release_id = release_detail.get("id").and_then(Value::as_str).unwrap_or("");
    let item_source = item.get("source").and_then(Value::as_str).unwrap_or("ad-hoc");

    match existing {
        None => json!({
            "action": "add",
            "id": item_id,
            "title": item_title,
            "targetReleaseId": release_id,
            "source": item_source
        }),
        Some(e) => {
            let e_target = e.get("targetReleaseId").and_then(Value::as_str).unwrap_or("");
            if e_target == release_id {
                json!({
                    "action": "unchanged",
                    "id": item_id,
                    "title": item_title,
                    "targetReleaseId": release_id,
                    "source": item.get("source").and_then(Value::as_str).unwrap_or("roadmap")
                })
            } else {
                json!({
                    "action": "retarget",
                    "id": item_id,
                    "title": item_title,
                    "fromReleaseId": e_target,
                    "targetReleaseId": release_id,
                    "source": item.get("source").and_then(Value::as_str).unwrap_or("roadmap")
                })
            }
        }
    }
}

/// Parse "major.minor.patch" semver into [u64; 3], None if format doesn't match.
fn parse_semver(s: &str) -> Option<[u64; 3]> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 { return None; }
    let maj = parts[0].parse::<u64>().ok()?;
    let min = parts[1].parse::<u64>().ok()?;
    let pat = parts[2].parse::<u64>().ok()?;
    Some([maj, min, pat])
}

fn str_or_null(obj: &Value, key: &str) -> Value {
    obj.get(key)
        .and_then(Value::as_str)
        .map(|s| Value::String(s.to_owned()))
        .unwrap_or(Value::Null)
}

/// `releaseSortKey(release)` — first non-null of releasedAt, targetDate, targetWindow, version, id.
fn release_sort_key(release: &Value) -> String {
    for field in &["releasedAt", "targetDate", "targetWindow", "version", "id"] {
        if let Some(v) = release.get(field).and_then(Value::as_str) {
            if !v.is_empty() {
                return v.to_owned();
            }
        }
    }
    String::new()
}

fn normalize_summary_for_compare(summary: &Value) -> Value {
    json!({
        "id": summary.get("id").and_then(Value::as_str).unwrap_or(""),
        "version": summary.get("version").and_then(Value::as_str).unwrap_or(""),
        "name": summary.get("name").and_then(Value::as_str).unwrap_or(""),
        "status": summary.get("status").and_then(Value::as_str).unwrap_or(""),
        "posture": summary.get("posture").and_then(Value::as_str).unwrap_or(""),
        "targetDate": summary.get("targetDate").cloned().unwrap_or(Value::Null),
        "targetWindow": summary.get("targetWindow").cloned().unwrap_or(Value::Null),
        "releasedAt": summary.get("releasedAt").cloned().unwrap_or(Value::Null),
        "lastUpdated": summary.get("lastUpdated").and_then(Value::as_str).unwrap_or(""),
        "summary": summary.get("summary").and_then(Value::as_str).unwrap_or(""),
        "counts": summary.get("counts").cloned().unwrap_or(Value::Null),
        "file": summary.get("file").and_then(Value::as_str).unwrap_or(""),
    })
}

fn same_summary(left: &Value, right: &Value) -> bool {
    serde_json::to_string(&normalize_summary_for_compare(left)).ok()
        == serde_json::to_string(&normalize_summary_for_compare(right)).ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_detail(scope_items: Vec<Value>) -> Value {
        json!({
            "id": "v1-0-0",
            "scope": {
                "required": scope_items,
                "planned": [],
                "stretch": [],
                "deferred": [],
                "outOfScope": []
            },
            "workstreams": [],
            "blockers": []
        })
    }

    #[test]
    fn release_items_order() {
        let detail = json!({
            "scope": {
                "required": [{"id":"r1"}],
                "planned": [{"id":"p1"}],
                "stretch": [{"id":"s1"}],
                "deferred": [{"id":"d1"}],
                "outOfScope": [{"id":"o1"}]
            }
        });
        let items = release_items(&detail);
        assert_eq!(items.len(), 5);
        assert_eq!(items[0]["id"], "r1");
        assert_eq!(items[4]["id"], "o1");
    }

    #[test]
    fn next_minor_version_empty() {
        assert_eq!(next_minor_version(&json!({"releases":[]})), json!("0.1.0"));
    }

    #[test]
    fn next_minor_version_normal() {
        let ri = json!({"releases":[{"version":"1.5.3"},{"version":"0.9.0"}]});
        assert_eq!(next_minor_version(&ri), json!("1.6.0"));
    }

    #[test]
    fn slug_basic() {
        assert_eq!(slug("Hello World!"), "hello-world");
        assert_eq!(slug("  leading trailing  "), "leading-trailing");
        assert_eq!(slug(""), "");
        assert_eq!(slug("---"), "");
    }

    #[test]
    fn unique_id_collision() {
        let mut used = std::collections::HashSet::new();
        assert_eq!(unique_id("Fix A", &mut used), "fix-a");
        assert_eq!(unique_id("Fix A", &mut used), "fix-a-2");
        assert_eq!(unique_id("Fix A", &mut used), "fix-a-3");
    }

    #[test]
    fn build_release_plan_basic() {
        let input = json!({
            "releaseIndex": {"releases":[{"version":"1.0.0"}]},
            "roadmapItems": [{"id":"fa","title":"Feat A","kind":"feature","summary":"S","section":"Core","status":"planned"}],
            "selectedRoadmapItemIds": ["fa"],
            "itemScopes": {},
            "adHocItems": [],
            "projectName": "TestApp",
            "now": "2024-01-01T00:00:00.000Z"
        });
        let result = build_release_plan(&input).unwrap();
        assert_eq!(result["id"], "v1-1-0");
        assert_eq!(result["scope"]["planned"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn build_release_plan_error_missing_now() {
        let input = json!({
            "releaseIndex": {"releases":[]},
            "roadmapItems": [],
            "selectedRoadmapItemIds": [],
            "projectName": "P",
            "version": "1.0.0"
        });
        let err = build_release_plan(&input).unwrap_err();
        assert_eq!(err, "buildReleasePlan requires an explicit now timestamp.");
    }

    #[test]
    fn build_release_plan_error_unknown_id() {
        let input = json!({
            "releaseIndex": {"releases":[]},
            "roadmapItems": [],
            "selectedRoadmapItemIds": ["unknown"],
            "projectName": "P",
            "version": "1.0.0",
            "now": "2024-01-01T00:00:00.000Z"
        });
        let err = build_release_plan(&input).unwrap_err();
        assert!(err.contains("unknown"));
    }

    #[test]
    fn merge_existing_id_mismatch() {
        let existing = json!({"id":"v0-9-0","status":"implementing","scope":{"required":[],"planned":[],"stretch":[],"deferred":[],"outOfScope":[]},"workstreams":[]});
        let proposed = json!({"id":"v1-0-0","status":"planned","scope":{"required":[],"planned":[],"stretch":[],"deferred":[],"outOfScope":[]},"workstreams":[]});
        let result = merge_existing_release_plan(&existing, &proposed);
        assert_eq!(result["id"], "v1-0-0");
        assert_eq!(result["status"], "planned");
    }
}
