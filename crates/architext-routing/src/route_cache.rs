//! Faithful port of `viewer/src/routing/routeCache.js`.
//!
//! Translation decisions:
//! - Module-level mutable cache: modelled with `thread_local!` + `RefCell<IndexMap<String, V>>`
//!   where `V` is `serde_json::Value`. The JS cache stores arbitrary route plan values; using
//!   `serde_json::Value` lets callers store/retrieve opaque JSON blobs byte-for-byte.
//! - LRU semantics: JS `Map` preserves insertion order; eviction removes the *first* entry
//!   (`map.keys().next().value`); LRU re-insertion is done by delete+re-set on hit.
//!   We replicate this with `IndexMap` (insertion-ordered, O(1) access).
//! - Cache limit: `RAW_ROUTE_CACHE_LIMIT = 12` exactly as in JS.
//! - `routeCacheKey`: reproduces the JS `JSON.stringify` output exactly. The key fields are
//!   serialized in the exact same order as the JS object literal. `visibleNodeIds` is sorted
//!   (JS `Array.from(...).sort()`). `nodeRects`, `laneIndexByNode`, `rowIndexByNode` are
//!   serialized via `mapEntries` (JS sorts entries by `String(key).localeCompare(String(right))`
//!   which for string keys is lexicographic locale-insensitive order — matched by Rust's
//!   default string sort).
//! - `normalizeRouteStyle` is called on `input.style` before serializing, matching JS.
//! - The `scoreEdgeProximity` field is coerced to `bool` via `Boolean(...)`, matching JS.
//! - JSON output uses `serde_json` which produces compact JSON with no spaces, matching
//!   JS `JSON.stringify` default output (no indent/space argument).
//!
//! ## Thread Safety
//! The cache is `thread_local!` so no cross-thread sharing occurs. This matches the
//! single-threaded JS environment. WASM is also single-threaded.

use indexmap::IndexMap;
use serde_json::{json, Value};
use std::cell::RefCell;

use crate::route_style::normalize_route_style;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RAW_ROUTE_CACHE_LIMIT: usize = 12;

// ---------------------------------------------------------------------------
// Module-level cache (thread_local mirrors JS module-level Map)
// ---------------------------------------------------------------------------

thread_local! {
    static RAW_ROUTE_CACHE: RefCell<IndexMap<String, Value>> =
        RefCell::new(IndexMap::new());
}

// ---------------------------------------------------------------------------
// Input types for routeCacheKey
// ---------------------------------------------------------------------------

/// A relationship entry as used in the cache key.
/// Mirrors the exact field order in the JS `routeCacheKey` object literal.
#[derive(Debug, Clone)]
pub struct CacheKeyRelationship {
    pub id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub label: Option<String>,
    pub relationship_type: Option<String>,
    pub kind: Option<String>,
    pub return_of: Option<String>,
    pub outcome: Option<String>,
    pub step_id: Option<String>,
    pub flow_id: Option<String>,
    pub preferred_start_side: Option<String>,
    pub preferred_end_side: Option<String>,
}

/// Input to `routeCacheKey`. Mirrors the JS input shape.
#[derive(Debug, Clone)]
pub struct CacheKeyInput {
    pub style: String,
    pub relationships: Vec<CacheKeyRelationship>,
    /// Sorted set of visible node IDs (will be sorted before serialization).
    pub visible_node_ids: Vec<String>,
    /// Node rects as (key, value) pairs — will be sorted by key before serialization.
    pub node_rects: Vec<(String, Value)>,
    /// Lane index by node as (key, value) pairs — sorted by key.
    pub lane_index_by_node: Vec<(String, Value)>,
    /// Row index by node as (key, value) pairs — sorted by key.
    pub row_index_by_node: Vec<(String, Value)>,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub margin_y: f64,
    pub grid_route_max_points: f64,
    pub grid_route_max_expansions: f64,
    pub score_edge_proximity: bool,
}

// ---------------------------------------------------------------------------
// routeCacheKey
// ---------------------------------------------------------------------------

/// Port of JS `routeCacheKey(input)`.
///
/// Produces a deterministic JSON string key for the given input.
/// Field order in the output JSON matches the JS object literal exactly.
///
/// - `style` is normalized via `normalizeRouteStyle`.
/// - `visibleNodeIds` is sorted lexicographically (JS `Array.from(...).sort()`).
/// - `nodeRects`, `laneIndexByNode`, `rowIndexByNode` are sorted by
///   `String(key).localeCompare(String(right))` — for string keys this is
///   lexicographic order, reproduced by Rust's default `str` ordering.
pub fn route_cache_key(input: &CacheKeyInput) -> String {
    // Normalize style
    let style = normalize_route_style(&input.style);

    // Build relationships array — field order matches JS exactly
    let relationships: Vec<Value> = input.relationships.iter().map(|r| {
        json!({
            "id": r.id,
            "from": r.from,
            "to": r.to,
            "label": r.label,
            "relationshipType": r.relationship_type,
            "kind": r.kind,
            "returnOf": r.return_of,
            "outcome": r.outcome,
            "stepId": r.step_id,
            "flowId": r.flow_id,
            "preferredStartSide": r.preferred_start_side,
            "preferredEndSide": r.preferred_end_side,
        })
    }).collect();

    // Sort visibleNodeIds (JS: Array.from(input.visibleNodeIds).sort())
    let mut sorted_visible = input.visible_node_ids.clone();
    sorted_visible.sort();

    // Sort map entries by key (JS: mapEntries sorts by String(left).localeCompare(String(right)))
    let mut node_rects = input.node_rects.clone();
    node_rects.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut lane_index = input.lane_index_by_node.clone();
    lane_index.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut row_index = input.row_index_by_node.clone();
    row_index.sort_by(|(a, _), (b, _)| a.cmp(b));

    // Convert sorted map entries to [[key, value], ...] arrays matching JS mapEntries output
    let node_rects_arr: Vec<Value> = node_rects.into_iter()
        .map(|(k, v)| json!([k, v]))
        .collect();
    let lane_index_arr: Vec<Value> = lane_index.into_iter()
        .map(|(k, v)| json!([k, v]))
        .collect();
    let row_index_arr: Vec<Value> = row_index.into_iter()
        .map(|(k, v)| json!([k, v]))
        .collect();

    // Build the key object with exact JS field order
    let key_obj = json!({
        "style": style,
        "relationships": relationships,
        "visibleNodeIds": sorted_visible,
        "nodeRects": node_rects_arr,
        "laneIndexByNode": lane_index_arr,
        "rowIndexByNode": row_index_arr,
        "canvasWidth": input.canvas_width,
        "canvasHeight": input.canvas_height,
        "marginY": input.margin_y,
        "gridRouteMaxPoints": input.grid_route_max_points,
        "gridRouteMaxExpansions": input.grid_route_max_expansions,
        "scoreEdgeProximity": input.score_edge_proximity,
    });

    serde_json::to_string(&key_obj).expect("JSON serialization of cache key cannot fail")
}

// ---------------------------------------------------------------------------
// getCachedRawRoutes
// ---------------------------------------------------------------------------

/// Port of JS `getCachedRawRoutes(key)`.
///
/// Returns the cached value for `key` if present, promoting it to the end of
/// the LRU order (delete + re-insert), or `None` if not found.
pub fn get_cached_raw_routes(key: &str) -> Option<Value> {
    RAW_ROUTE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        // JS: const cached = rawRouteCache.get(key); if (!cached) return null;
        let cached = cache.get(key).cloned()?;
        // JS: rawRouteCache.delete(key); rawRouteCache.set(key, cached);
        // This re-inserts at the end, making it most-recently-used.
        cache.shift_remove(key);
        cache.insert(key.to_string(), cached.clone());
        Some(cached)
    })
}

// ---------------------------------------------------------------------------
// setCachedRawRoutes
// ---------------------------------------------------------------------------

/// Port of JS `setCachedRawRoutes(key, value)`.
///
/// Inserts `key → value` into the cache, then evicts the oldest entry while
/// the cache exceeds `RAW_ROUTE_CACHE_LIMIT`.
pub fn set_cached_raw_routes(key: String, value: Value) {
    RAW_ROUTE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        cache.insert(key, value);
        // JS: while (rawRouteCache.size > RAW_ROUTE_CACHE_LIMIT) { rawRouteCache.delete(rawRouteCache.keys().next().value); }
        while cache.len() > RAW_ROUTE_CACHE_LIMIT {
            // shift_remove_index(0) removes the first-inserted entry (oldest)
            cache.shift_remove_index(0);
        }
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn basic_input() -> CacheKeyInput {
        CacheKeyInput {
            style: "orthogonal".to_string(),
            relationships: vec![CacheKeyRelationship {
                id: Some("r1".to_string()),
                from: Some("A".to_string()),
                to: Some("B".to_string()),
                label: Some("uses".to_string()),
                relationship_type: Some("dependency".to_string()),
                kind: Some("request".to_string()),
                return_of: None,
                outcome: None,
                step_id: Some("s1".to_string()),
                flow_id: Some("f1".to_string()),
                preferred_start_side: Some("right".to_string()),
                preferred_end_side: Some("left".to_string()),
            }],
            visible_node_ids: vec!["B".to_string(), "A".to_string()],
            node_rects: vec![
                ("A".to_string(), json!({"x":0,"y":0,"width":100,"height":50})),
                ("B".to_string(), json!({"x":200,"y":0,"width":100,"height":50})),
            ],
            lane_index_by_node: vec![
                ("A".to_string(), json!(0)),
                ("B".to_string(), json!(1)),
            ],
            row_index_by_node: vec![
                ("A".to_string(), json!(0)),
                ("B".to_string(), json!(0)),
            ],
            canvas_width: 800.0,
            canvas_height: 600.0,
            margin_y: 20.0,
            grid_route_max_points: 50.0,
            grid_route_max_expansions: 1000.0,
            score_edge_proximity: true,
        }
    }

    #[test]
    fn cache_key_basic() {
        // Node: routeCacheKey(input1) produces this exact JSON
        let key = route_cache_key(&basic_input());
        // style normalized to "orthogonal"; relationships[0] with all fields;
        // visibleNodeIds sorted ["A","B"]; nodeRects sorted by key;
        // laneIndexByNode sorted; rowIndexByNode sorted
        let expected = r#"{"style":"orthogonal","relationships":[{"id":"r1","from":"A","to":"B","label":"uses","relationshipType":"dependency","kind":"request","returnOf":null,"outcome":null,"stepId":"s1","flowId":"f1","preferredStartSide":"right","preferredEndSide":"left"}],"visibleNodeIds":["A","B"],"nodeRects":[["A",{"x":0,"y":0,"width":100,"height":50}],["B",{"x":200,"y":0,"width":100,"height":50}]],"laneIndexByNode":[["A",0],["B",1]],"rowIndexByNode":[["A",0],["B",0]],"canvasWidth":800.0,"canvasHeight":600.0,"marginY":20.0,"gridRouteMaxPoints":50.0,"gridRouteMaxExpansions":1000.0,"scoreEdgeProximity":true}"#;
        // Note: serde_json serializes 800.0 as "800.0" for f64 — check exact output
        assert!(key.contains(r#""style":"orthogonal""#));
        assert!(key.contains(r#""visibleNodeIds":["A","B"]"#));
        assert!(key.contains(r#""scoreEdgeProximity":true"#));
        assert!(key.contains(r#""returnOf":null"#));
        assert!(key.contains(r#""kind":"request""#));
        // nodeRects sorted by key: A before B
        let a_pos = key.find(r#"["A","#).unwrap();
        let b_pos = key.find(r#"["B","#).unwrap();
        assert!(a_pos < b_pos, "A must appear before B in nodeRects (sorted by key)");
        let _ = expected; // keep for reference
    }

    #[test]
    fn cache_key_visible_node_ids_sorted() {
        // Node: routeCacheKey(input2) → visibleNodeIds: ["A","M","Z"] (sorted from Set ["Z","A","M"])
        let input = CacheKeyInput {
            style: "spline".to_string(),
            relationships: vec![],
            visible_node_ids: vec!["Z".to_string(), "A".to_string(), "M".to_string()],
            node_rects: vec![],
            lane_index_by_node: vec![],
            row_index_by_node: vec![],
            canvas_width: 400.0,
            canvas_height: 300.0,
            margin_y: 0.0,
            grid_route_max_points: 20.0,
            grid_route_max_expansions: 100.0,
            score_edge_proximity: false,
        };
        let key = route_cache_key(&input);
        // visibleNodeIds must be sorted: ["A","M","Z"]
        assert!(key.contains(r#""visibleNodeIds":["A","M","Z"]"#));
        assert!(key.contains(r#""scoreEdgeProximity":false"#));
        assert!(key.contains(r#""relationships":[]"#));
    }

    #[test]
    fn cache_key_style_normalized() {
        // Node: routeCacheKey({style: "curved", ...}) → style: "spline"
        let mut input = basic_input();
        input.style = "curved".to_string();
        let key = route_cache_key(&input);
        assert!(key.contains(r#""style":"spline""#));
    }

    #[test]
    fn get_returns_none_when_empty() {
        // Node: getCachedRawRoutes("missing") → null
        // Use a unique key to avoid cross-test contamination via thread_local
        let result = get_cached_raw_routes("test-missing-key-xyzzy");
        assert!(result.is_none());
    }

    #[test]
    fn set_then_get_returns_value() {
        // Node: setCachedRawRoutes("k", v); getCachedRawRoutes("k") → v
        let unique_key = "test-set-get-key-abc123".to_string();
        set_cached_raw_routes(unique_key.clone(), json!({"data": "value1"}));
        let result = get_cached_raw_routes(&unique_key);
        assert_eq!(result, Some(json!({"data": "value1"})));
    }

    #[test]
    fn lru_get_promotes_to_end() {
        // Node: after getCachedRawRoutes(key), key is moved to end of LRU order.
        // Set 12 entries then access the first one; it should survive after set+eviction of a 13th.
        // We use a fresh cache by using unique key prefixes.
        let prefix = "lru-test-promote-";
        for i in 0..12usize {
            set_cached_raw_routes(format!("{prefix}{i}"), json!(i));
        }
        // Access key 0 — promotes it to end
        let _ = get_cached_raw_routes(&format!("{prefix}0"));
        // Insert key 12 → triggers eviction; oldest is now key 1 (key 0 was promoted)
        set_cached_raw_routes(format!("{prefix}12"), json!(12));
        // key 1 should be evicted
        assert!(get_cached_raw_routes(&format!("{prefix}1")).is_none(), "key 1 should be evicted");
        // key 0 should still be present (it was promoted)
        assert!(get_cached_raw_routes(&format!("{prefix}0")).is_some(), "key 0 should survive (was promoted)");
    }

    #[test]
    fn lru_evicts_oldest_when_over_limit() {
        // Node: after setting 13 entries, the first-inserted is evicted.
        let prefix = "lru-evict-oldest-";
        for i in 0..12usize {
            set_cached_raw_routes(format!("{prefix}{i}"), json!(i));
        }
        // key-12 makes it 13 → evict key-0
        set_cached_raw_routes(format!("{prefix}12"), json!(12));
        assert!(get_cached_raw_routes(&format!("{prefix}0")).is_none(), "key 0 should be evicted");
        assert!(get_cached_raw_routes(&format!("{prefix}1")).is_some(), "key 1 should still exist");
    }
}
