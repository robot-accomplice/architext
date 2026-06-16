//! Port of `viewer/src/routing/planKey.js`.
//!
//! `plan_input_key(input)` returns the canonical JSON string that uniquely
//! identifies a plan request — the exact string sha256'd to produce the cache key.
//!
//! CRITICAL V8 SEMANTICS reproduced here:
//! - `JSON.stringify` DROPS keys whose value is `undefined`, KEEPS `null`.
//!   In practice: `relationship.label`, `preferredStartSide`, `preferredEndSide`
//!   and other Option<String> fields are serialized as `null` when Some, and
//!   OMITTED when None (i.e. when the JS value is `undefined`).
//! - Numbers use V8's `Number::toString` → `js_number_to_string`.
//! - `sortedMapEntries` sorts by `String(left).localeCompare(String(right))` → `js_locale_compare`.
//! - `visibleNodeIds` sorted by default `.sort()` → `js_default_sort_cmp`.
//! - `roundRect` uses `Math.round` → `js_round` (applied to rect coordinates).

use crate::js_compat::{js_default_sort_cmp, js_locale_compare, js_number_to_string, js_round};

/// Wire shape of the relationship as used in the plan key.
/// Fields that can be `undefined` in JS are `Option<String>` here —
/// they are OMITTED from the JSON output when `None`.
#[derive(Debug, Clone)]
pub struct PlanKeyRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    /// JS: `relationship.label` — omitted if undefined, null if null, string if string.
    /// In practice the JS buildFlowRelationships always sets label to a string
    /// (the "N. action" format), so this is always Some.
    pub label: Option<String>,
    pub relationship_type: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub display_index: i64,
    /// `undefined` when not set (i.e. not a decision branch) → omitted from JSON.
    pub preferred_start_side: Option<String>,
    /// `undefined` when not set → omitted from JSON.
    pub preferred_end_side: Option<String>,
}

/// A lane in the view (used in the key's `view.lanes` array).
pub struct PlanKeyLane<'a> {
    pub id: &'a str,
    pub node_ids: &'a [String],
}

/// An extra node rect entry (sorted by key in the output).
pub struct PlanKeyExtraRect {
    pub node_id: String,
    /// [x, y, width, height] after Math.round — as integer values.
    pub rounded: [i64; 4],
}

/// An extra index entry (laneIndex or rowIndex by node).
pub struct PlanKeyExtraIndex {
    pub node_id: String,
    pub value: i64,
}

/// The full input needed to produce the plan key string.
pub struct PlanKeyInput<'a> {
    pub view_id: &'a str,
    pub view_type: &'a str,
    pub lanes: &'a [PlanKeyLane<'a>],
    pub relationships: &'a [PlanKeyRelationship],
    pub visible_node_ids: &'a [String], // already sorted by caller (js_default_sort_cmp)
    pub node_width: f64,
    pub node_height: f64,
    pub lane_width: f64,
    pub row_gap: f64,
    pub margin_x: f64,
    pub margin_y: f64,
    pub min_canvas_width: f64,
    pub min_canvas_height: f64,
    pub canvas_extra_width: f64,
    pub canvas_extra_height: f64,
    pub extra_node_rects: &'a [PlanKeyExtraRect],        // sorted by node_id (locale compare)
    pub extra_lane_index_by_node: &'a [PlanKeyExtraIndex], // sorted by node_id (locale compare)
    pub extra_row_index_by_node: &'a [PlanKeyExtraIndex],  // sorted by node_id (locale compare)
    pub score_edge_proximity: bool,
    pub style: &'a str,
}

// ---------------------------------------------------------------------------
// JSON-serialisation helpers that exactly reproduce V8's JSON.stringify output
// ---------------------------------------------------------------------------

/// Append a JSON string value (with quotes and escaping).
fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Append a JSON number value using V8's `Number::toString` (via js_number_to_string).
fn push_num(out: &mut String, n: f64) {
    out.push_str(&js_number_to_string(n));
}

/// Append a JSON boolean.
fn push_bool(out: &mut String, b: bool) {
    out.push_str(if b { "true" } else { "false" });
}

/// Append `"key":` (key is always an ASCII-safe string in our domain).
fn push_key(out: &mut String, key: &str) {
    push_str(out, key);
    out.push(':');
}

/// Append an optional string field:
/// - `None` → omit the field entirely (JS `undefined` → JSON.stringify drops it)
/// - `Some(s)` → `"key":"value"`
///
/// Note: this function does NOT push the leading comma; the caller handles commas.
fn push_opt_str_field_if_some(out: &mut String, key: &str, value: &Option<String>, is_first: &mut bool) {
    if let Some(s) = value {
        if !*is_first { out.push(','); }
        push_key(out, key);
        push_str(out, s);
        *is_first = false;
    }
    // If None → skip entirely (JS undefined behaviour)
}

/// Append a required string field with leading comma if not first. Returns false (new is_first value).
fn push_str_field(out: &mut String, key: &str, value: &str, is_first: bool) -> bool {
    if !is_first { out.push(','); }
    push_key(out, key);
    push_str(out, value);
    false
}

/// Append a required i64 field with leading comma if not first. Returns false.
fn push_i64_field(out: &mut String, key: &str, value: i64, is_first: bool) -> bool {
    if !is_first { out.push(','); }
    push_key(out, key);
    out.push_str(&value.to_string());
    false
}

/// Build the canonical plan-input key string — a byte-exact port of JS
/// `planInputKey(input)` → `JSON.stringify({...})`.
pub fn plan_input_key(input: &PlanKeyInput<'_>) -> String {
    let mut out = String::with_capacity(4096);
    out.push('{');

    // --- view ---
    {
        push_key(&mut out, "view");
        out.push('{');

        // view.id
        push_str_field(&mut out, "id", input.view_id, true);
        // view.type
        push_str_field(&mut out, "type", input.view_type, false);
        // view.lanes
        out.push_str(",\"lanes\":[");
        for (i, lane) in input.lanes.iter().enumerate() {
            if i > 0 { out.push(','); }
            out.push('[');
            push_str(&mut out, lane.id);
            out.push(',');
            // nodeIds array
            out.push('[');
            for (j, nid) in lane.node_ids.iter().enumerate() {
                if j > 0 { out.push(','); }
                push_str(&mut out, nid);
            }
            out.push(']');
            out.push(']');
        }
        out.push(']');
        out.push('}');
    }

    // --- relationships ---
    out.push_str(",\"relationships\":[");
    for (i, rel) in input.relationships.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('{');
        let mut first = true;

        // id, from, to are required strings
        first = push_str_field(&mut out, "id", &rel.id, first);
        first = push_str_field(&mut out, "from", &rel.from, first);
        first = push_str_field(&mut out, "to", &rel.to, first);

        // label: optional (undefined → omit, string → emit)
        push_opt_str_field_if_some(&mut out, "label", &rel.label, &mut first);

        // relationshipType: optional
        push_opt_str_field_if_some(&mut out, "relationshipType", &rel.relationship_type, &mut first);

        // stepId: optional
        push_opt_str_field_if_some(&mut out, "stepId", &rel.step_id, &mut first);

        // flowId: optional
        push_opt_str_field_if_some(&mut out, "flowId", &rel.flow_id, &mut first);

        // kind: optional
        push_opt_str_field_if_some(&mut out, "kind", &rel.kind, &mut first);

        // returnOf: optional
        push_opt_str_field_if_some(&mut out, "returnOf", &rel.return_of, &mut first);

        // outcome: optional
        push_opt_str_field_if_some(&mut out, "outcome", &rel.outcome, &mut first);

        // displayIndex: required number (integer)
        first = push_i64_field(&mut out, "displayIndex", rel.display_index, first);

        // preferredStartSide: optional (undefined → omit)
        push_opt_str_field_if_some(&mut out, "preferredStartSide", &rel.preferred_start_side, &mut first);

        // preferredEndSide: optional (undefined → omit)
        push_opt_str_field_if_some(&mut out, "preferredEndSide", &rel.preferred_end_side, &mut first);

        let _ = first;
        out.push('}');
    }
    out.push(']');

    // --- visibleNodeIds (pre-sorted by caller) ---
    out.push_str(",\"visibleNodeIds\":[");
    for (i, nid) in input.visible_node_ids.iter().enumerate() {
        if i > 0 { out.push(','); }
        push_str(&mut out, nid);
    }
    out.push(']');

    // --- scalar layout fields ---
    // Emit each as ,"key":value (all preceded by comma since relationships/visibleNodeIds came first).
    for (key, val) in [
        ("nodeWidth",       input.node_width),
        ("nodeHeight",      input.node_height),
        ("laneWidth",       input.lane_width),
        ("rowGap",          input.row_gap),
        ("marginX",         input.margin_x),
        ("marginY",         input.margin_y),
        ("minCanvasWidth",  input.min_canvas_width),
        ("minCanvasHeight", input.min_canvas_height),
        ("canvasExtraWidth", input.canvas_extra_width),
        ("canvasExtraHeight", input.canvas_extra_height),
    ] {
        out.push(',');
        push_key(&mut out, key);
        push_num(&mut out, val);
    }

    // --- extraNodeRects (sorted by key via locale compare, applied before this call) ---
    out.push_str(",\"extraNodeRects\":[");
    for (i, entry) in input.extra_node_rects.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('[');
        push_str(&mut out, &entry.node_id);
        out.push_str(",[");
        out.push_str(&entry.rounded[0].to_string());
        out.push(',');
        out.push_str(&entry.rounded[1].to_string());
        out.push(',');
        out.push_str(&entry.rounded[2].to_string());
        out.push(',');
        out.push_str(&entry.rounded[3].to_string());
        out.push_str("]]");
    }
    out.push(']');

    // --- extraLaneIndexByNode ---
    out.push_str(",\"extraLaneIndexByNode\":[");
    for (i, entry) in input.extra_lane_index_by_node.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('[');
        push_str(&mut out, &entry.node_id);
        out.push(',');
        out.push_str(&entry.value.to_string());
        out.push(']');
    }
    out.push(']');

    // --- extraRowIndexByNode ---
    out.push_str(",\"extraRowIndexByNode\":[");
    for (i, entry) in input.extra_row_index_by_node.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('[');
        push_str(&mut out, &entry.node_id);
        out.push(',');
        out.push_str(&entry.value.to_string());
        out.push(']');
    }
    out.push(']');

    // --- scoreEdgeProximity ---
    out.push_str(",\"scoreEdgeProximity\":");
    push_bool(&mut out, input.score_edge_proximity);

    // --- style ---
    out.push_str(",\"style\":");
    push_str(&mut out, input.style);

    out.push('}');
    out
}

/// Sort a list of (node_id, value) entries by key using `js_locale_compare`.
pub fn sorted_map_entries<V: Clone>(entries: &mut [(String, V)]) {
    entries.sort_by(|(a, _), (b, _)| js_locale_compare(a, b));
}

/// Sort a list of strings using `js_default_sort_cmp`.
pub fn sorted_visible_node_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = ids.into_iter().collect();
    v.sort_by(|a, b| js_default_sort_cmp(a, b));
    v
}

/// Apply `js_round` to rect coordinates and return [x, y, width, height].
pub fn round_rect(x: f64, y: f64, width: f64, height: f64) -> [i64; 4] {
    [
        js_round(x) as i64,
        js_round(y) as i64,
        js_round(width) as i64,
        js_round(height) as i64,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_key_structure() {
        let input = PlanKeyInput {
            view_id: "v1",
            view_type: "system-map",
            lanes: &[PlanKeyLane { id: "l0", node_ids: &["a".to_string()] }],
            relationships: &[],
            visible_node_ids: &["a".to_string()],
            node_width: 136.0,
            node_height: 54.0,
            lane_width: 210.0,
            row_gap: 102.0,
            margin_x: 180.0,
            margin_y: 104.0,
            min_canvas_width: 0.0,
            min_canvas_height: 340.0,
            canvas_extra_width: 132.0,
            canvas_extra_height: 88.0,
            extra_node_rects: &[],
            extra_lane_index_by_node: &[],
            extra_row_index_by_node: &[],
            score_edge_proximity: false,
            style: "orthogonal",
        };
        let key = plan_input_key(&input);
        // Spot-check key is valid JSON containing expected fields
        let parsed: serde_json::Value = serde_json::from_str(&key).expect("key should be valid JSON");
        assert_eq!(parsed["view"]["id"], "v1");
        assert_eq!(parsed["view"]["type"], "system-map");
        assert_eq!(parsed["nodeWidth"], 136);
        assert_eq!(parsed["style"], "orthogonal");
    }

    #[test]
    fn undefined_fields_are_dropped() {
        // A relationship with no optional fields should not include them in the key.
        let rel = PlanKeyRelationship {
            id: "r1".to_string(),
            from: "a".to_string(),
            to: "b".to_string(),
            label: Some("1. do".to_string()),
            relationship_type: Some("flow".to_string()),
            step_id: Some("s1".to_string()),
            flow_id: Some("f1".to_string()),
            kind: None,       // undefined in JS → omitted
            return_of: None,  // undefined → omitted
            outcome: None,    // undefined → omitted
            display_index: 1,
            preferred_start_side: None, // undefined → omitted
            preferred_end_side: None,   // undefined → omitted
        };
        let input = PlanKeyInput {
            view_id: "v1",
            view_type: "system-map",
            lanes: &[PlanKeyLane { id: "l0", node_ids: &["a".to_string(), "b".to_string()] }],
            relationships: &[rel],
            visible_node_ids: &["a".to_string(), "b".to_string()],
            node_width: 136.0,
            node_height: 54.0,
            lane_width: 210.0,
            row_gap: 102.0,
            margin_x: 180.0,
            margin_y: 104.0,
            min_canvas_width: 0.0,
            min_canvas_height: 340.0,
            canvas_extra_width: 132.0,
            canvas_extra_height: 88.0,
            extra_node_rects: &[],
            extra_lane_index_by_node: &[],
            extra_row_index_by_node: &[],
            score_edge_proximity: false,
            style: "orthogonal",
        };
        let key = plan_input_key(&input);
        let parsed: serde_json::Value = serde_json::from_str(&key).expect("valid JSON");
        let rel_json = &parsed["relationships"][0];
        // These are Some → present
        assert_eq!(rel_json["id"], "r1");
        assert_eq!(rel_json["label"], "1. do");
        // These are None → must not appear
        assert!(rel_json.get("kind").is_none(), "kind should be absent when None");
        assert!(rel_json.get("preferredStartSide").is_none(), "preferredStartSide should be absent when None");
        assert!(rel_json.get("preferredEndSide").is_none(), "preferredEndSide should be absent when None");
    }

    #[test]
    fn number_formatting_uses_js_semantics() {
        // 0.1 + 0.2 = 0.30000000000000004 in V8
        let input = PlanKeyInput {
            view_id: "v1",
            view_type: "system-map",
            lanes: &[],
            relationships: &[],
            visible_node_ids: &[],
            node_width: 0.1 + 0.2, // should produce "0.30000000000000004"
            node_height: 54.0,
            lane_width: 210.0,
            row_gap: 102.0,
            margin_x: 180.0,
            margin_y: 104.0,
            min_canvas_width: 0.0,
            min_canvas_height: 340.0,
            canvas_extra_width: 132.0,
            canvas_extra_height: 88.0,
            extra_node_rects: &[],
            extra_lane_index_by_node: &[],
            extra_row_index_by_node: &[],
            score_edge_proximity: false,
            style: "orthogonal",
        };
        let key = plan_input_key(&input);
        assert!(key.contains("\"nodeWidth\":0.30000000000000004"), "key should contain V8 number: {key}");
    }

    #[test]
    fn round_rect_uses_js_round() {
        // Math.round is half-toward-+inf
        assert_eq!(round_rect(2.5, 3.5, 37.5, 37.5), [3, 4, 38, 38]);
    }

    #[test]
    fn sorted_visible_node_ids_uses_js_sort() {
        let ids = vec!["s10".to_string(), "s2".to_string(), "s1".to_string()];
        let sorted = sorted_visible_node_ids(ids);
        assert_eq!(sorted, vec!["s1", "s10", "s2"]);
    }
}
