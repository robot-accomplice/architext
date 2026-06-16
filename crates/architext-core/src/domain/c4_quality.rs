//! Pure port of `src/domain/architecture-model/c4-quality.mjs`.
//!
//! All functions operate on `serde_json::Value` (preserve_order enabled).
//! Insertion-order iteration is preserved via `IndexMap`/`IndexSet` wherever
//! the JS source iterates a `Map` or `Set` whose order influences output.

use indexmap::{IndexMap, IndexSet};
use serde_json::{json, Value};

// ── Const tables ──────────────────────────────────────────────────────────────

fn c4_type_expectations(view_type: &str) -> Option<&'static [&'static str]> {
    match view_type {
        "c4-context" => Some(&[
            "actor", "client", "service", "deployment-unit", "external-service",
            "software-system", "trust-boundary",
        ]),
        "c4-container" => Some(&[
            "actor", "client", "service", "worker", "data-store", "queue",
            "deployment-unit", "external-service", "software-system",
        ]),
        "c4-component" => Some(&[
            "actor", "client", "service", "module", "worker", "data-store",
            "queue", "external-service",
        ]),
        _ => None,
    }
}

fn c4_density_budget(view_type: &str) -> Option<(u64, u64)> {
    // returns (nodes_budget, relationships_budget)
    match view_type {
        "c4-context" => Some((14, 18)),
        "c4-container" => Some((14, 24)),
        "c4-component" => Some((14, 28)),
        _ => None,
    }
}

fn c4_drilldown_type(view_type: &str) -> Option<&'static str> {
    match view_type {
        "c4-context" => Some("c4-container"),
        "c4-container" => Some("c4-component"),
        "c4-component" => Some("c4-code"),
        _ => None,
    }
}

fn c4_decomposable_types() -> &'static [&'static str] {
    &["software-system", "service", "worker", "deployment-unit", "client"]
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// `viewNodeIds(view)` = `view.lanes.flatMap(lane => lane.nodeIds)`.
/// NOT deduped — duplicates are intentionally counted.
fn view_node_ids(view: &Value) -> Vec<String> {
    view["lanes"]
        .as_array()
        .map(|lanes| {
            lanes.iter().flat_map(|lane| {
                lane["nodeIds"]
                    .as_array()
                    .map(|ids| {
                        ids.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            }).collect()
        })
        .unwrap_or_default()
}

/// `uniqueId(base, usedIds)` — collision-suffix: `base`, `base-2`, `base-3`, …
/// Mutates `usedIds` by inserting the chosen id.
fn unique_id(base: &str, used_ids: &mut IndexSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut index = 2usize;
    while used_ids.contains(&candidate) {
        candidate = format!("{base}-{index}");
        index += 1;
    }
    used_ids.insert(candidate.clone());
    candidate
}

/// `structuralRelationshipCount(view, nodeMap)`.
fn structural_relationship_count(
    view: &Value,
    node_map: &IndexMap<String, Value>,
) -> u64 {
    let visible: IndexSet<String> = view_node_ids(view).into_iter().collect();
    let mut count = 0u64;
    for node_id in &visible {
        let node = node_map.get(node_id);
        let deps = node
            .and_then(|n| n["dependencies"].as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for dep in deps {
            if let Some(dep_id) = dep.as_str() {
                if visible.contains(dep_id) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// `duplicateViewNodes(view)` — returns nodeIds that appear more than once,
/// in insertion order of first encounter (Map iteration order).
fn duplicate_view_nodes(view: &Value) -> Vec<String> {
    let mut counts: IndexMap<String, u32> = IndexMap::new();
    for node_id in view_node_ids(view) {
        *counts.entry(node_id).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(node_id, _)| node_id)
        .collect()
}

/// `dedupeC4View(view)` — returns `{ view, removed }`.
fn dedupe_c4_view(view: &Value) -> (Value, u32) {
    let mut seen: IndexSet<String> = IndexSet::new();
    let mut removed = 0u32;
    let lanes = view["lanes"]
        .as_array()
        .map(|lanes| {
            lanes.iter().map(|lane| {
                let node_ids: Vec<Value> = lane["nodeIds"]
                    .as_array()
                    .map(|ids| {
                        ids.iter().filter_map(|v| {
                            let id = v.as_str()?.to_string();
                            if seen.contains(&id) {
                                removed += 1;
                                None
                            } else {
                                seen.insert(id.clone());
                                Some(Value::String(id))
                            }
                        }).collect()
                    })
                    .unwrap_or_default();
                // { ...lane, nodeIds }
                let mut new_lane = lane.clone();
                new_lane.as_object_mut().unwrap()
                    .insert("nodeIds".to_string(), Value::Array(node_ids));
                new_lane
            }).collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut new_view = view.clone();
    new_view.as_object_mut().unwrap()
        .insert("lanes".to_string(), Value::Array(lanes));
    (new_view, removed)
}

/// `connectedToFocus(nodeId, focusIds, nodeMap)`.
fn connected_to_focus(
    node_id: &str,
    focus_ids: &IndexSet<String>,
    node_map: &IndexMap<String, Value>,
) -> bool {
    // node?.dependencies.some(depId => focusIds.has(depId))
    if let Some(node) = node_map.get(node_id) {
        if let Some(deps) = node["dependencies"].as_array() {
            if deps.iter().any(|d| {
                d.as_str().map(|s| focus_ids.contains(s)).unwrap_or(false)
            }) {
                return true;
            }
        }
    }
    // focusIds.some(focusId => nodeMap.get(focusId)?.dependencies.includes(nodeId))
    for focus_id in focus_ids {
        if let Some(focus_node) = node_map.get(focus_id) {
            if let Some(deps) = focus_node["dependencies"].as_array() {
                if deps.iter().any(|d| d.as_str() == Some(node_id)) {
                    return true;
                }
            }
        }
    }
    false
}

/// `viewWithIds(view, nodeIds)` — filter lanes to only include the given ids;
/// drop empty lanes.
fn view_with_ids(view: &Value, node_ids: &IndexSet<String>) -> Value {
    let lanes: Vec<Value> = view["lanes"]
        .as_array()
        .map(|lanes| {
            lanes.iter().filter_map(|lane| {
                let filtered_ids: Vec<Value> = lane["nodeIds"]
                    .as_array()
                    .map(|ids| {
                        ids.iter()
                            .filter_map(|v| v.as_str())
                            .filter(|id| node_ids.contains(*id))
                            .map(|id| Value::String(id.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                if filtered_ids.is_empty() {
                    return None;
                }
                let mut new_lane = lane.clone();
                new_lane.as_object_mut().unwrap()
                    .insert("nodeIds".to_string(), Value::Array(filtered_ids));
                Some(new_lane)
            }).collect()
        })
        .unwrap_or_default();

    let mut new_view = view.clone();
    new_view.as_object_mut().unwrap()
        .insert("lanes".to_string(), Value::Array(lanes));
    new_view
}

/// `splitDenseC4View(view, nodeMap, usedViewIds)`.
fn split_dense_c4_view(
    view: &Value,
    node_map: &IndexMap<String, Value>,
    used_view_ids: &mut IndexSet<String>,
) -> Vec<Value> {
    let budget = match c4_density_budget(view["type"].as_str().unwrap_or("")) {
        Some(b) => b,
        None => return vec![],
    };
    let lanes = match view["lanes"].as_array() {
        Some(l) if l.len() >= 2 => l,
        _ => return vec![],
    };

    let mut splits: Vec<Value> = Vec::new();

    for focus_lane in lanes {
        let focus_node_ids: Vec<String> = focus_lane["nodeIds"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        if focus_node_ids.is_empty() {
            continue;
        }

        // selected = new Set(focusLane.nodeIds.slice(0, budget.nodes))
        let mut selected: IndexSet<String> = focus_node_ids
            .iter()
            .take(budget.0 as usize)
            .cloned()
            .collect();
        let focus_ids: IndexSet<String> = selected.clone();

        // candidates = other lanes' nodeIds filtered to those connected to focus
        let focus_lane_id = focus_lane["id"].as_str().unwrap_or("");
        let candidates: Vec<String> = view["lanes"]
            .as_array()
            .map(|all_lanes| {
                all_lanes.iter()
                    .filter(|lane| lane["id"].as_str() != Some(focus_lane_id))
                    .flat_map(|lane| {
                        lane["nodeIds"]
                            .as_array()
                            .map(|ids| {
                                ids.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    })
                    .filter(|node_id| connected_to_focus(node_id, &focus_ids, node_map))
                    .collect()
            })
            .unwrap_or_default();

        for candidate in candidates {
            if selected.contains(&candidate) || selected.len() >= budget.0 as usize {
                continue;
            }
            let mut trial_ids = selected.clone();
            trial_ids.insert(candidate.clone());
            let trial = view_with_ids(view, &trial_ids);
            if structural_relationship_count(&trial, node_map) <= budget.1 {
                selected.insert(candidate);
            }
        }

        let view_id = view["id"].as_str().unwrap_or("");
        let focus_lane_id_str = focus_lane["id"].as_str().unwrap_or("");
        let base = format!("{view_id}-{focus_lane_id_str}");
        let id = unique_id(&base, used_view_ids);

        let focus_lane_name = focus_lane["name"].as_str().unwrap_or("");
        let view_name = view["name"].as_str().unwrap_or("");
        let view_summary = view["summary"].as_str().unwrap_or("");

        let mut split_view = view_with_ids(view, &selected);
        let split_obj = split_view.as_object_mut().unwrap();
        split_obj.insert("id".to_string(), Value::String(id));
        split_obj.insert(
            "name".to_string(),
            Value::String(format!("{view_name}: {focus_lane_name}")),
        );
        split_obj.insert(
            "summary".to_string(),
            Value::String(format!(
                "{view_summary} Focused on {focus_lane_name}; generated by architext sync without changing architecture facts."
            )),
        );
        // Note: "type" is already in split_view from view_with_ids (it clones view)
        // We need to make sure the field insertion order for id/name/summary matches JS spread.
        // JS: { ...viewWithIds(view, selected), id, name, summary }
        // This means id/name/summary OVERWRITE existing keys (at end in JS spread? No - spread sets them
        // at their original position if existing, or appends if new). In serde_json with preserve_order,
        // `insert` on existing key updates in-place. Let's verify the JS spread order doesn't matter
        // for semantic equality (the parity gate uses object-key-order-insensitive comparison).
        splits.push(split_view);
    }

    if splits.len() > 1 {
        splits
    } else {
        vec![]
    }
}

// ── Build node map from `nodes` array ────────────────────────────────────────

/// Build `IndexMap<nodeId, node>` from a `nodes` JSON array.
/// Last occurrence wins (matches JS `new Map(nodes.map(n => [n.id, n]))`).
pub fn build_node_map(nodes: &[Value]) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for node in nodes {
        if let Some(id) = node["id"].as_str() {
            map.insert(id.to_string(), node.clone());
        }
    }
    map
}

// ── Public API ────────────────────────────────────────────────────────────────

/// `c4IssuesForView(view, nodeMap)` — returns array of issue strings.
pub fn c4_issues_for_view(view: &Value, node_map: &IndexMap<String, Value>) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();
    let view_id = view["id"].as_str().unwrap_or("");
    let view_type = view["type"].as_str().unwrap_or("");
    let allowed_types = c4_type_expectations(view_type);
    let budget = c4_density_budget(view_type);
    let duplicates = duplicate_view_nodes(view);

    if !duplicates.is_empty() {
        issues.push(format!(
            "{view_id}: duplicate node membership: {}",
            duplicates.join(", ")
        ));
    }

    if allowed_types.is_none() {
        issues.push(format!("{view_id}: unsupported C4 view type {view_type}"));
    }

    for node_id in view_node_ids(view) {
        let node = node_map.get(&node_id);
        if node.is_none() {
            issues.push(format!("{view_id}: missing node {node_id}"));
        } else if let (Some(allowed), Some(node)) = (allowed_types, node) {
            let node_type = node["type"].as_str().unwrap_or("");
            if !allowed.contains(&node_type) {
                issues.push(format!(
                    "{view_id}: {node_id} has {node_type}, which does not belong in {view_type}"
                ));
            }
        }
    }

    if let Some((node_budget, rel_budget)) = budget {
        let node_count = {
            let ids: IndexSet<String> = view_node_ids(view).into_iter().collect();
            ids.len() as u64
        };
        let rel_count = structural_relationship_count(view, node_map);
        if node_count > node_budget {
            issues.push(format!(
                "{view_id}: {node_count} nodes exceeds {node_budget}; split the view"
            ));
        }
        if rel_count > rel_budget {
            issues.push(format!(
                "{view_id}: {rel_count} relationships exceeds {rel_budget}; split the view"
            ));
        }
    }

    issues
}

/// `c4DrilldownIssues(views, nodeMap)` — returns array of issue strings.
pub fn c4_drilldown_issues(views: &[Value], node_map: &IndexMap<String, Value>) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();

    for view in views {
        let child_type = match c4_drilldown_type(view["type"].as_str().unwrap_or("")) {
            Some(ct) => ct,
            None => continue,
        };
        let view_id = view["id"].as_str().unwrap_or("");

        for node_id in view_node_ids(view) {
            let node = match node_map.get(&node_id) {
                Some(n) => n,
                None => continue,
            };
            let node_type = node["type"].as_str().unwrap_or("");
            if !c4_decomposable_types().contains(&node_type) {
                continue;
            }
            let has_child = views.iter().any(|candidate| {
                candidate["type"].as_str() == Some(child_type)
                    && candidate["scopeNodeId"].as_str() == Some(&node_id)
            });
            if !has_child {
                issues.push(format!(
                    "{view_id}: {node_id} has no {child_type} drilldown view"
                ));
            }
        }
    }

    issues
}

/// `repairC4Views(views, nodeMap)` — returns `{ views, changes }`.
pub fn repair_c4_views(views: &[Value], node_map: &IndexMap<String, Value>) -> Value {
    let mut used_view_ids: IndexSet<String> = views
        .iter()
        .filter_map(|v| v["id"].as_str().map(|s| s.to_string()))
        .collect();

    let mut next_views: Vec<Value> = Vec::new();
    let mut changes: Vec<String> = Vec::new();

    for view in views {
        let view_type = view["type"].as_str().unwrap_or("");
        if !view_type.starts_with("c4-") {
            next_views.push(view.clone());
            continue;
        }

        let before_duplicates = duplicate_view_nodes(view);
        let (deduped_view, removed) = dedupe_c4_view(view);
        let view_id = view["id"].as_str().unwrap_or("");

        if removed > 0 {
            let entry_word = if removed == 1 { "entry" } else { "entries" };
            changes.push(format!(
                "{view_id}: remove {removed} duplicate node membership {entry_word} ({})",
                before_duplicates.join(", ")
            ));
        }

        let budget = c4_density_budget(deduped_view["type"].as_str().unwrap_or(""));
        let node_count = {
            let ids: IndexSet<String> = view_node_ids(&deduped_view).into_iter().collect();
            ids.len() as u64
        };
        let rel_count = structural_relationship_count(&deduped_view, node_map);

        if let Some((node_budget, rel_budget)) = budget {
            if node_count > node_budget || rel_count > rel_budget {
                let splits = split_dense_c4_view(&deduped_view, node_map, &mut used_view_ids);
                if !splits.is_empty() {
                    changes.push(format!(
                        "{view_id}: split dense {} view into {} scoped views",
                        deduped_view["type"].as_str().unwrap_or(""),
                        splits.len()
                    ));
                    next_views.extend(splits);
                    continue;
                }
            }
        }

        next_views.push(deduped_view);
    }

    json!({
        "views": next_views,
        "changes": changes
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_map(nodes: &[Value]) -> IndexMap<String, Value> {
        build_node_map(nodes)
    }

    fn simple_view(id: &str, view_type: &str, node_ids: Vec<&str>) -> Value {
        json!({
            "id": id,
            "type": view_type,
            "name": "Test",
            "lanes": [{ "id": "l1", "name": "L1", "nodeIds": node_ids }]
        })
    }

    #[test]
    fn issues_clean_view() {
        let view = simple_view("v1", "c4-context", vec!["n1"]);
        let nodes = vec![json!({"id": "n1", "type": "actor"})];
        let map = make_map(&nodes);
        assert_eq!(c4_issues_for_view(&view, &map), Vec::<String>::new());
    }

    #[test]
    fn issues_duplicate_membership() {
        let view = simple_view("v2", "c4-context", vec!["n1", "n1"]);
        let nodes = vec![json!({"id": "n1", "type": "actor"})];
        let map = make_map(&nodes);
        let issues = c4_issues_for_view(&view, &map);
        assert!(issues[0].contains("duplicate node membership: n1"));
    }

    #[test]
    fn issues_unsupported_view_type() {
        let view = simple_view("v3", "c4-code", vec!["n1"]);
        let nodes = vec![json!({"id": "n1", "type": "actor"})];
        let map = make_map(&nodes);
        let issues = c4_issues_for_view(&view, &map);
        assert!(issues.iter().any(|i| i.contains("unsupported C4 view type c4-code")));
    }

    #[test]
    fn issues_missing_node() {
        let view = simple_view("v4", "c4-context", vec!["n1", "nMissing"]);
        let nodes = vec![json!({"id": "n1", "type": "actor"})];
        let map = make_map(&nodes);
        let issues = c4_issues_for_view(&view, &map);
        assert!(issues.iter().any(|i| i.contains("missing node nMissing")));
    }

    #[test]
    fn issues_wrong_node_type() {
        let view = simple_view("v5", "c4-context", vec!["n3"]);
        let nodes = vec![json!({"id": "n3", "type": "module"})];
        let map = make_map(&nodes);
        let issues = c4_issues_for_view(&view, &map);
        assert!(issues.iter().any(|i| i.contains("does not belong in c4-context")));
    }

    #[test]
    fn unique_id_no_collision() {
        let mut used = IndexSet::new();
        assert_eq!(unique_id("base", &mut used), "base");
        assert!(used.contains("base"));
    }

    #[test]
    fn unique_id_with_collision() {
        let mut used: IndexSet<String> = ["base".to_string()].into_iter().collect();
        assert_eq!(unique_id("base", &mut used), "base-2");
        assert_eq!(unique_id("base", &mut used), "base-3");
    }

    #[test]
    fn drilldown_no_issues_when_child_present() {
        let views = vec![
            json!({
                "id": "v1", "type": "c4-context", "name": "Ctx",
                "lanes": [{"id":"l1","name":"L1","nodeIds":["n1"]}]
            }),
            json!({
                "id": "v2", "type": "c4-container", "name": "Ctn",
                "scopeNodeId": "n1",
                "lanes": [{"id":"l1","name":"L1","nodeIds":["n2"]}]
            }),
        ];
        let nodes = vec![
            json!({"id":"n1","type":"software-system"}),
            json!({"id":"n2","type":"module"}),
        ];
        let map = make_map(&nodes);
        // n2 is module (not decomposable), n1 has child → no issues
        let issues = c4_drilldown_issues(&views, &map);
        assert!(issues.is_empty());
    }

    #[test]
    fn drilldown_missing_child_issue() {
        let views = vec![
            json!({
                "id": "v1", "type": "c4-context", "name": "Ctx",
                "lanes": [{"id":"l1","name":"L1","nodeIds":["n1"]}]
            })
        ];
        let nodes = vec![json!({"id":"n1","type":"software-system"})];
        let map = make_map(&nodes);
        let issues = c4_drilldown_issues(&views, &map);
        assert_eq!(issues, vec!["v1: n1 has no c4-container drilldown view"]);
    }

    #[test]
    fn repair_non_c4_passthrough() {
        let views = vec![json!({
            "id": "v1", "type": "system-map", "name": "Map",
            "lanes": [{"id":"l1","name":"L1","nodeIds":["n1"]}]
        })];
        let map = make_map(&[]);
        let result = repair_c4_views(&views, &map);
        assert_eq!(result["changes"].as_array().unwrap().len(), 0);
        assert_eq!(result["views"][0]["id"], "v1");
    }

    #[test]
    fn repair_dedupe_singular() {
        let views = vec![json!({
            "id": "v1", "type": "c4-context", "name": "Ctx",
            "lanes": [{"id":"l1","name":"L1","nodeIds":["n1","n1"]}]
        })];
        let nodes = vec![json!({"id":"n1","type":"actor"})];
        let map = make_map(&nodes);
        let result = repair_c4_views(&views, &map);
        let changes = result["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].as_str().unwrap().contains("entry"));
        assert!(!changes[0].as_str().unwrap().contains("entries"));
    }

    #[test]
    fn repair_dedupe_plural() {
        let views = vec![json!({
            "id": "v1", "type": "c4-context", "name": "Ctx",
            "lanes": [{"id":"l1","name":"L1","nodeIds":["n1","n1","n2","n2"]}]
        })];
        let nodes = vec![
            json!({"id":"n1","type":"actor"}),
            json!({"id":"n2","type":"service"}),
        ];
        let map = make_map(&nodes);
        let result = repair_c4_views(&views, &map);
        let changes = result["changes"].as_array().unwrap();
        assert!(changes[0].as_str().unwrap().contains("entries"));
    }
}
