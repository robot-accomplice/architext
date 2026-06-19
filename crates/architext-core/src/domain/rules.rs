//! Pure port of `src/domain/architecture-model/rules.mjs`.
//!
//! All functions operate on `serde_json::Value` (preserve_order enabled) so
//! passthrough keys and object fields keep insertion order, exactly as the
//! JS spread operator does.

use serde_json::Value;
use architext_routing::js_compat::js_locale_compare;

/// Map criticality string → sort rank (unknown → 99).
fn criticality_rank(c: &str) -> u32 {
    match c {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 99,
    }
}

/// JS `rule.protection.edit || rule.protection.delete`
fn protected_from_reorder(rule: &Value) -> bool {
    let p = &rule["protection"];
    p["edit"].as_bool().unwrap_or(false) || p["delete"].as_bool().unwrap_or(false)
}

/// `orderedRules(rules)` — stable sort matching the JS comparator.
///
/// Primary: criticalityRank (unknown → 99).
/// Secondary: numeric `order` field.
/// Tertiary: localeCompare on `title`.
pub fn ordered_rules(rules: &[Value]) -> Vec<Value> {
    let mut out = rules.to_vec();
    out.sort_by(|left, right| {
        let lc = criticality_rank(left["criticality"].as_str().unwrap_or(""));
        let rc = criticality_rank(right["criticality"].as_str().unwrap_or(""));
        if lc != rc {
            return lc.cmp(&rc);
        }
        let lo = left["order"].as_f64().unwrap_or(0.0);
        let ro = right["order"].as_f64().unwrap_or(0.0);
        if lo != ro {
            return lo.partial_cmp(&ro).unwrap_or(std::cmp::Ordering::Equal);
        }
        let lt = left["title"].as_str().unwrap_or("");
        let rt = right["title"].as_str().unwrap_or("");
        js_locale_compare(lt, rt)
    });
    out
}

/// `upsertRule(rulesDocument, rule)` — insert or update; throws if edit-protected.
///
/// Returns `Err(message)` on violation; `Ok(updated_document)` otherwise.
pub fn upsert_rule(rules_document: &Value, rule: &Value) -> Result<Value, String> {
    let id = rule["id"].as_str().unwrap_or("");
    let rules_arr = rules_document["rules"].as_array()
        .ok_or_else(|| "rules must be an array".to_string())?;

    let existing = rules_arr.iter().find(|c| c["id"].as_str() == Some(id));

    if let Some(e) = existing {
        if e["protection"]["edit"].as_bool().unwrap_or(false) {
            return Err(format!("Rule \"{id}\" is edit protected"));
        }
    }

    let next_rule = if let Some(e) = existing {
        // { ...existing, ...rule, protection: rule.protection ?? existing.protection }
        let mut merged = e.clone();
        // Spread rule fields over existing
        if let Some(rule_obj) = rule.as_object() {
            let merged_obj = merged.as_object_mut().unwrap();
            for (k, v) in rule_obj {
                merged_obj.insert(k.clone(), v.clone());
            }
        }
        // protection: rule.protection ?? existing.protection
        if rule["protection"].is_null() || rule.get("protection").is_none() {
            let ep = e["protection"].clone();
            merged.as_object_mut().unwrap().insert("protection".to_string(), ep);
        }
        merged
    } else {
        rule.clone()
    };

    let new_rules: Vec<Value> = if existing.is_some() {
        rules_arr.iter().map(|c| {
            if c["id"].as_str() == Some(id) { next_rule.clone() } else { c.clone() }
        }).collect()
    } else {
        let mut v = rules_arr.clone();
        v.push(next_rule);
        v
    };

    let mut out = rules_document.clone();
    out.as_object_mut().unwrap().insert("rules".to_string(), Value::Array(new_rules));
    Ok(out)
}

/// `deleteRule(rulesDocument, id)`.
pub fn delete_rule(rules_document: &Value, id: &str) -> Result<Value, String> {
    let rules_arr = rules_document["rules"].as_array()
        .ok_or_else(|| "rules must be an array".to_string())?;

    let existing = rules_arr.iter().find(|c| c["id"].as_str() == Some(id));
    if existing.is_none() {
        return Err(format!("Rule \"{id}\" was not found"));
    }
    let existing = existing.unwrap();
    if existing["protection"]["delete"].as_bool().unwrap_or(false) {
        return Err(format!("Rule \"{id}\" is delete protected"));
    }

    let new_rules: Vec<Value> = rules_arr.iter()
        .filter(|c| c["id"].as_str() != Some(id))
        .cloned()
        .collect();

    let mut out = rules_document.clone();
    out.as_object_mut().unwrap().insert("rules".to_string(), Value::Array(new_rules));
    Ok(out)
}

/// `moveRule(rulesDocument, id, direction)` — "up" or "down" within criticality tier.
pub fn move_rule(rules_document: &Value, id: &str, direction: &str) -> Result<Value, String> {
    let rules_arr = rules_document["rules"].as_array()
        .ok_or_else(|| "rules must be an array".to_string())?;

    let existing = rules_arr.iter().find(|c| c["id"].as_str() == Some(id));
    if existing.is_none() {
        return Err(format!("Rule \"{id}\" was not found"));
    }
    let existing = existing.unwrap();
    if protected_from_reorder(existing) {
        return Err(format!("Rule \"{id}\" is protected from reordering"));
    }

    let existing_criticality = existing["criticality"].as_str().unwrap_or("");
    let peers: Vec<Value> = ordered_rules(rules_arr)
        .into_iter()
        .filter(|c| {
            c["criticality"].as_str().unwrap_or("") == existing_criticality
                && !protected_from_reorder(c)
        })
        .collect();

    let index = peers.iter().position(|c| c["id"].as_str() == Some(id));
    let index = match index {
        Some(i) => i,
        None => return Ok(rules_document.clone()),
    };

    let target_index = if direction == "up" {
        if index == 0 { return Ok(rules_document.clone()); }
        index - 1
    } else {
        index + 1
    };

    let target = match peers.get(target_index) {
        Some(t) => t,
        None => return Ok(rules_document.clone()),
    };

    let existing_order = existing["order"].clone();
    let target_order = target["order"].clone();
    let existing_id = existing["id"].as_str().unwrap_or("").to_string();
    let target_id = target["id"].as_str().unwrap_or("").to_string();

    let new_rules: Vec<Value> = rules_arr.iter().map(|c| {
        if c["id"].as_str() == Some(&existing_id) {
            let mut r = c.clone();
            r.as_object_mut().unwrap().insert("order".to_string(), target_order.clone());
            r
        } else if c["id"].as_str() == Some(&target_id) {
            let mut r = c.clone();
            r.as_object_mut().unwrap().insert("order".to_string(), existing_order.clone());
            r
        } else {
            c.clone()
        }
    }).collect();

    let mut out = rules_document.clone();
    out.as_object_mut().unwrap().insert("rules".to_string(), Value::Array(new_rules));
    Ok(out)
}

/// `moveRuleBefore(rulesDocument, id, beforeId)`.
pub fn move_rule_before(rules_document: &Value, id: &str, before_id: &str) -> Result<Value, String> {
    if id == before_id {
        return Ok(rules_document.clone());
    }
    let rules_arr = rules_document["rules"].as_array()
        .ok_or_else(|| "rules must be an array".to_string())?;

    let existing = rules_arr.iter().find(|c| c["id"].as_str() == Some(id));
    let target = rules_arr.iter().find(|c| c["id"].as_str() == Some(before_id));

    if existing.is_none() {
        return Err(format!("Rule \"{id}\" was not found"));
    }
    if target.is_none() {
        return Err(format!("Rule \"{before_id}\" was not found"));
    }
    let existing = existing.unwrap();
    let target = target.unwrap();

    if protected_from_reorder(existing) {
        return Err(format!("Rule \"{id}\" is protected from reordering"));
    }
    if protected_from_reorder(target) {
        return Err(format!("Rule \"{before_id}\" is protected from reordering"));
    }
    let existing_criticality = existing["criticality"].as_str().unwrap_or("");
    let target_criticality = target["criticality"].as_str().unwrap_or("");
    if existing_criticality != target_criticality {
        return Err("Rules can only be reordered within the same criticality group".to_string());
    }

    let peers: Vec<Value> = ordered_rules(rules_arr)
        .into_iter()
        .filter(|c| {
            c["criticality"].as_str().unwrap_or("") == existing_criticality
                && !protected_from_reorder(c)
        })
        .collect();

    // order slots sorted numerically (JS `.sort((l,r) => l - r)`)
    let mut order_slots: Vec<f64> = peers.iter()
        .map(|c| c["order"].as_f64().unwrap_or(0.0))
        .collect();
    order_slots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // reordered = peers without `id`, then splice `existing` before `beforeId`
    let mut reordered: Vec<Value> = peers.iter()
        .filter(|c| c["id"].as_str() != Some(id))
        .cloned()
        .collect();
    let target_idx = reordered.iter().position(|c| c["id"].as_str() == Some(before_id))
        .unwrap_or(reordered.len());
    reordered.insert(target_idx, existing.clone());

    // Map id → order slot
    let order_by_id: std::collections::HashMap<String, f64> = reordered.iter()
        .enumerate()
        .map(|(i, c)| (c["id"].as_str().unwrap_or("").to_string(), order_slots[i]))
        .collect();

    let new_rules: Vec<Value> = rules_arr.iter().map(|c| {
        let cid = c["id"].as_str().unwrap_or("");
        if let Some(&slot) = order_by_id.get(cid) {
            let mut r = c.clone();
            r.as_object_mut().unwrap().insert("order".to_string(), Value::Number(
                serde_json::Number::from_f64(slot).unwrap()
            ));
            r
        } else {
            c.clone()
        }
    }).collect();

    let mut out = rules_document.clone();
    out.as_object_mut().unwrap().insert("rules".to_string(), Value::Array(new_rules));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(id: &str, criticality: &str, order: i64, title: &str) -> Value {
        json!({
            "id": id,
            "title": title,
            "criticality": criticality,
            "order": order,
            "protection": { "edit": false, "delete": false }
        })
    }

    fn protected_rule(id: &str, edit: bool, del: bool) -> Value {
        json!({
            "id": id,
            "title": id,
            "criticality": "high",
            "order": 1,
            "protection": { "edit": edit, "delete": del }
        })
    }

    #[test]
    fn ordered_rules_sorts_by_criticality_then_order_then_title() {
        let rules = vec![
            rule("c", "low", 1, "C"),
            rule("b", "high", 2, "B"),
            rule("a", "high", 1, "A"),
            rule("u", "unknown", 1, "U"),
            rule("m", "medium", 1, "M"),
            rule("x", "critical", 1, "X"),
        ];
        let sorted = ordered_rules(&rules);
        let ids: Vec<&str> = sorted.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["x", "a", "b", "m", "c", "u"]);
    }

    #[test]
    fn ordered_rules_title_tiebreak() {
        let rules = vec![
            rule("z", "high", 1, "Zebra"),
            rule("a", "high", 1, "Aardvark"),
        ];
        let sorted = ordered_rules(&rules);
        assert_eq!(sorted[0]["id"], "a");
        assert_eq!(sorted[1]["id"], "z");
    }

    #[test]
    fn upsert_inserts_new_rule() {
        let doc = json!({ "rules": [] });
        let r = rule("r1", "high", 1, "Rule1");
        let out = upsert_rule(&doc, &r).unwrap();
        assert_eq!(out["rules"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn upsert_updates_existing_rule() {
        let doc = json!({ "rules": [rule("r1", "high", 1, "Rule1")] });
        let updated = json!({
            "id": "r1",
            "title": "Rule1 Updated",
            "criticality": "high",
            "order": 1,
            "protection": { "edit": false, "delete": false }
        });
        let out = upsert_rule(&doc, &updated).unwrap();
        assert_eq!(out["rules"][0]["title"], "Rule1 Updated");
    }

    #[test]
    fn upsert_rejects_edit_protected() {
        let doc = json!({ "rules": [protected_rule("r1", true, false)] });
        let r = json!({ "id": "r1", "title": "X", "protection": { "edit": true, "delete": false } });
        let err = upsert_rule(&doc, &r).unwrap_err();
        assert_eq!(err, "Rule \"r1\" is edit protected");
    }

    #[test]
    fn delete_removes_rule() {
        let doc = json!({ "rules": [rule("r1", "high", 1, "R1")] });
        let out = delete_rule(&doc, "r1").unwrap();
        assert!(out["rules"].as_array().unwrap().is_empty());
    }

    #[test]
    fn delete_not_found_error() {
        let doc = json!({ "rules": [] });
        let err = delete_rule(&doc, "missing").unwrap_err();
        assert_eq!(err, "Rule \"missing\" was not found");
    }

    #[test]
    fn delete_protected_error() {
        let doc = json!({ "rules": [protected_rule("r1", false, true)] });
        let err = delete_rule(&doc, "r1").unwrap_err();
        assert_eq!(err, "Rule \"r1\" is delete protected");
    }

    #[test]
    fn move_rule_up_swaps_orders() {
        let doc = json!({ "rules": [
            rule("a", "high", 1, "A"),
            rule("b", "high", 2, "B"),
        ]});
        let out = move_rule(&doc, "b", "up").unwrap();
        let rules = out["rules"].as_array().unwrap();
        let a = rules.iter().find(|r| r["id"] == "a").unwrap();
        let b = rules.iter().find(|r| r["id"] == "b").unwrap();
        assert_eq!(a["order"], 2);
        assert_eq!(b["order"], 1);
    }

    #[test]
    fn move_rule_at_boundary_returns_unchanged() {
        let doc = json!({ "rules": [rule("a", "high", 1, "A")] });
        let out = move_rule(&doc, "a", "up").unwrap();
        assert_eq!(out, doc);
    }

    #[test]
    fn move_rule_protected_error() {
        let doc = json!({ "rules": [protected_rule("r1", true, false)] });
        let err = move_rule(&doc, "r1", "up").unwrap_err();
        assert_eq!(err, "Rule \"r1\" is protected from reordering");
    }

    #[test]
    fn move_rule_before_reorders_within_tier() {
        let doc = json!({ "rules": [
            rule("a", "high", 10, "A"),
            rule("b", "high", 20, "B"),
            rule("c", "high", 30, "C"),
        ]});
        // move c before a → order: c=10, a=20, b=30
        let out = move_rule_before(&doc, "c", "a").unwrap();
        let rules = out["rules"].as_array().unwrap();
        let a = rules.iter().find(|r| r["id"] == "a").unwrap();
        let b = rules.iter().find(|r| r["id"] == "b").unwrap();
        let c = rules.iter().find(|r| r["id"] == "c").unwrap();
        assert_eq!(c["order"].as_f64(), Some(10.0));
        assert_eq!(a["order"].as_f64(), Some(20.0));
        assert_eq!(b["order"].as_f64(), Some(30.0));
    }

    #[test]
    fn move_rule_before_same_id_noop() {
        let doc = json!({ "rules": [rule("a", "high", 1, "A")] });
        let out = move_rule_before(&doc, "a", "a").unwrap();
        assert_eq!(out, doc);
    }

    #[test]
    fn move_rule_before_cross_tier_error() {
        let doc = json!({ "rules": [
            rule("a", "high", 1, "A"),
            rule("b", "medium", 1, "B"),
        ]});
        let err = move_rule_before(&doc, "a", "b").unwrap_err();
        assert_eq!(err, "Rules can only be reordered within the same criticality group");
    }

    #[test]
    fn move_rule_before_protected_source_error() {
        let doc = json!({ "rules": [
            protected_rule("r1", true, false),
            rule("r2", "high", 2, "R2"),
        ]});
        let err = move_rule_before(&doc, "r1", "r2").unwrap_err();
        assert_eq!(err, "Rule \"r1\" is protected from reordering");
    }

    #[test]
    fn move_rule_before_protected_target_error() {
        let doc = json!({ "rules": [
            rule("r1", "high", 1, "R1"),
            protected_rule("r2", false, true),
        ]});
        let err = move_rule_before(&doc, "r1", "r2").unwrap_err();
        assert_eq!(err, "Rule \"r2\" is protected from reordering");
    }
}
