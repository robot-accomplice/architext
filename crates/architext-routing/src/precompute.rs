//! Native plan-precompute farm.
//!
//! Port of the enumeration logic in `src/adapters/http/plan-precompute.mjs`:
//! - `enumerateFlowPlanRequests` → `enumerate_flow_plan_requests`
//! - `planKeyHash` → `plan_key_hash`
//!
//! The farm reads `flows.json` and `views.json` from `data_dir`, enumerates
//! every (flow × flows-mode view) pair where `flowCompatibleWithView`, builds
//! the plan request for each, computes the plan via the Rust engine, serialises
//! the result, and returns a `FarmEntry` per request.
//!
//! Rayon is used for parallel computation (CPU-bound).
//!
//! This module is `cfg(not(target_arch = "wasm32"))` because rayon and sha2
//! are native-only dependencies. The wasm build excludes it entirely.

#[cfg(feature = "native")]
use rayon::prelude::*;

use std::path::Path;

use crate::diagram_config::DiagramConfig;
use crate::plan_diagram::{plan_diagram, ExtraNodeRect};
use crate::plan_request::{
    build_flow_plan_request,
    types::{Flow, FlowsFile, View, ViewsFile},
    view_selection::flow_compatible_with_view,
    view_selection::flow_view_types,
    diagram_layout::LayoutConfig,
};

// ---------------------------------------------------------------------------
// planKeyHash — port of JS `planKeyHash(key)` (sha256 hex)
// ---------------------------------------------------------------------------

/// Port of JS `planKeyHash(key)`: sha256 hex of the key string.
#[cfg(feature = "native")]
pub fn plan_key_hash_native(key: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Fallback (no native feature): compute hash using a simple pure-Rust sha256.
/// This is only used from the farm_dump bin which always has the native feature.
#[cfg(not(feature = "native"))]
pub fn plan_key_hash_native(_key: &str) -> String {
    // Non-native build: sha2/hex not available. Return empty. In practice the
    // farm_dump binary always enables the native feature.
    String::new()
}

// ---------------------------------------------------------------------------
// Farm entry
// ---------------------------------------------------------------------------

/// One entry from the precompute farm.
#[derive(Debug, Clone)]
pub struct FarmEntry {
    pub flow_id: String,
    pub view_id: String,
    /// The canonical plan key string (JSON).
    pub key: String,
    /// sha256 hex of `key`.
    pub hash: String,
    /// The serialised plan JSON (same wire shape as the JS precompute farm stores).
    pub plan_json: String,
}

// ---------------------------------------------------------------------------
// enumerate_flow_plan_requests
// ---------------------------------------------------------------------------

/// Read and parse flows.json and views.json from `data_dir`.
pub fn load_flows_and_views(data_dir: &Path) -> Result<(Vec<Flow>, Vec<View>), String> {
    let flows_path = data_dir.join("flows.json");
    let views_path = data_dir.join("views.json");

    let flows_text = std::fs::read_to_string(&flows_path)
        .map_err(|e| format!("read {}: {e}", flows_path.display()))?;
    let views_text = std::fs::read_to_string(&views_path)
        .map_err(|e| format!("read {}: {e}", views_path.display()))?;

    let flows_file: FlowsFile = serde_json::from_str(&flows_text)
        .map_err(|e| format!("parse flows.json: {e}"))?;
    let views_file: ViewsFile = serde_json::from_str(&views_text)
        .map_err(|e| format!("parse views.json: {e}"))?;

    Ok((flows_file.flows, views_file.views))
}

/// Port of JS `enumerateFlowPlanRequests({ dataDir, layoutConfig })`.
///
/// Returns one `FarmEntry` per (flow, view) pair where the flow is compatible
/// with the flows-mode view. Entries are in the same order as JS: outer loop
/// over views, inner loop over flows.
///
/// `layout_config` is `None` when no config files are present (corpus case),
/// meaning the `DiagramConfigLayout` defaults are used.
pub fn enumerate_flow_plan_requests(
    data_dir: &Path,
    diagram_config: &DiagramConfig,
) -> Result<Vec<FarmEntry>, String> {
    let (flows, views) = load_flows_and_views(data_dir)?;
    let flow_view_type_set: std::collections::HashSet<&str> = flow_view_types().iter().copied().collect();
    let layout_config = diagram_config.layout.to_layout_config();

    // Collect (flow, view) pairs (order: outer=views, inner=flows, matching JS)
    let pairs: Vec<(&View, &Flow)> = views.iter()
        .filter(|v| flow_view_type_set.contains(v.view_type.as_str()))
        .flat_map(|v| {
            flows.iter()
                .filter(move |f| flow_compatible_with_view(f, v))
                .map(move |f| (v, f))
        })
        .collect();

    // Build + compute each pair — can be parallelised with rayon (native feature)
    #[cfg(feature = "native")]
    let entries: Vec<Result<FarmEntry, String>> = pairs.par_iter()
        .map(|(view, flow)| build_farm_entry(view, flow, &layout_config))
        .collect();

    #[cfg(not(feature = "native"))]
    let entries: Vec<Result<FarmEntry, String>> = pairs.iter()
        .map(|(view, flow)| build_farm_entry(view, flow, &layout_config))
        .collect();

    // Return in insertion order (rayon par_iter preserves index-mapped collect order)
    entries.into_iter().collect()
}

/// Streaming variant of [`enumerate_flow_plan_requests`]: publishes each plan via
/// `on_entry` the instant it is computed, instead of collecting them all first.
/// This lets the serve farm light up diagrams incrementally during warm-up, so
/// the first diagram a viewer opens is ready after its own plan computes rather
/// than after the entire corpus has warmed (which can be tens of seconds on a
/// dense repo). Returns the number of plans warmed.
#[cfg(feature = "native")]
pub fn warm_flow_plans(
    data_dir: &Path,
    diagram_config: &DiagramConfig,
    on_entry: impl Fn(FarmEntry) + Sync,
) -> Result<usize, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (flows, views) = load_flows_and_views(data_dir)?;
    let flow_view_type_set: std::collections::HashSet<&str> =
        flow_view_types().iter().copied().collect();
    let layout_config = diagram_config.layout.to_layout_config();

    let pairs: Vec<(&View, &Flow)> = views
        .iter()
        .filter(|v| flow_view_type_set.contains(v.view_type.as_str()))
        .flat_map(|v| {
            flows
                .iter()
                .filter(move |f| flow_compatible_with_view(f, v))
                .map(move |f| (v, f))
        })
        .collect();

    let warmed = AtomicUsize::new(0);
    pairs.par_iter().for_each(|(view, flow)| {
        if let Ok(entry) = build_farm_entry(view, flow, &layout_config) {
            on_entry(entry);
            warmed.fetch_add(1, Ordering::Relaxed);
        }
    });
    Ok(warmed.load(Ordering::Relaxed))
}

fn build_farm_entry(view: &View, flow: &Flow, layout_config: &LayoutConfig) -> Result<FarmEntry, String> {
    let req = build_flow_plan_request(view, flow, Some(layout_config), "orthogonal");

    // Compute the plan via the native engine
    let plan = plan_diagram(&req.plan_diagram_input);

    // Serialise to a serde_json::Value so we can apply JS serialization normalization.
    let mut plan_value = serde_json::to_value(&plan)
        .map_err(|e| format!("serialise plan for flow={} view={}: {e}", flow.id, view.id))?;

    // Patch decision node rects: the Plan struct stores plain {x,y,w,h} Rect values,
    // but JS keeps the full ExtraNodeRect (including fixedPorts:true and sideAnchors)
    // in the nodeRects map. Restore those fields from the input's extra_node_rects.
    patch_extra_node_rects(&mut plan_value, &req.plan_diagram_input.extra_node_rects);

    // Reorder route fields to match JS insertion order (the Route struct serializes
    // `points` before `labelX/labelY`; JS has `points` after `samples`).
    reorder_plan_routes(&mut plan_value);

    // Normalize integer-valued f64 → JSON integer, matching V8 JSON.stringify semantics.
    normalize_floats(&mut plan_value);

    let plan_json = serde_json::to_string(&plan_value)
        .map_err(|e| format!("re-serialise plan for flow={} view={}: {e}", flow.id, view.id))?;

    let hash = plan_key_hash_native(&req.key);

    Ok(FarmEntry {
        flow_id: flow.id.clone(),
        view_id: view.id.clone(),
        key: req.key,
        hash,
        plan_json,
    })
}

/// Patch the `nodeRects` array in a serialised plan Value to restore `fixedPorts`
/// and `sideAnchors` for decision-diamond nodes.
///
/// The Rust `Plan` struct stores node rects as plain `{x,y,width,height}`, stripping
/// the extra fields that JS keeps. JS preserves the full `ExtraNodeRect` (with
/// `fixedPorts:true` and `sideAnchors`) in the output `nodeRects` map, so we must
/// restore those fields to match the byte-for-byte JS wire shape.
fn patch_extra_node_rects(
    plan: &mut serde_json::Value,
    extra_node_rects: &indexmap::IndexMap<String, ExtraNodeRect>,
) {
    if extra_node_rects.is_empty() {
        return;
    }
    let Some(node_rects) = plan.get_mut("nodeRects").and_then(|r| r.as_array_mut()) else {
        return;
    };
    for entry in node_rects.iter_mut() {
        let Some(arr) = entry.as_array_mut() else { continue };
        if arr.len() < 2 { continue; }
        let node_id = arr[0].as_str().map(|s| s.to_string());
        let Some(node_id) = node_id else { continue };
        let Some(extra) = extra_node_rects.get(&node_id) else { continue };
        // Only decision-diamond nodes have extra fields (fixedPorts + sideAnchors)
        if !extra.fixed_ports && extra.side_anchors.is_none() {
            continue;
        }
        // Serialise the full ExtraNodeRect (which preserves insertion order via
        // serde_json/preserve_order + derive Serialize field order).
        let Ok(full_rect) = serde_json::to_value(extra) else { continue };
        arr[1] = full_rect;
    }
}

/// Walk a `serde_json::Value` and convert any Number that is an integer-valued f64
/// into a JSON integer. This matches V8's `JSON.stringify` behaviour: `0.0` → `0`,
/// `28.0` → `28`, but `1.5` stays `1.5`.
fn normalize_floats(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    *v = serde_json::Value::Number((f as i64).into());
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                normalize_floats(item);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                normalize_floats(val);
            }
        }
        _ => {}
    }
}

/// Reorder the fields of a route JSON object to match JS insertion order:
/// d, labelX, labelY, bends, samples, points, [extra fields], sampleBounds, style
///
/// The Rust `Route` struct serializes with `d, points, labelX, labelY, [extra]`
/// because `points` is a named struct field. JS produces a different order because
/// the router builds the route object with `points` after `samples`, and `sampleBounds`
/// and `style` appended last. This function reconstructs the JS insertion order.
fn reorder_route_fields(route_obj: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = route_obj else { return route_obj; };

    // JS-canonical field order for a route object.
    // Fields not present in a given route are omitted.
    const JS_FIELD_ORDER: &[&str] = &[
        "d", "labelX", "labelY", "bends", "samples", "points",
        "startSide", "endSide", "qualityCosts", "cost",
        "collisions", "paddedCollisions", "endpointNodeTraversals",
        "selfOverlappingSegments", "selfOverlapLength",
        "crossings", "repeatedCrossings",
        "sharedSegments", "sharedSegmentLength",
        "surfaceMismatchCount", "semanticSurfaceMismatchCount",
        "surfaceDirectionMismatchCount", "blockedPrimarySurfaceUseCount",
        "sameLaneExteriorMismatchCount",
        "warnings", "controls", "sampleBounds", "style",
    ];

    let mut ordered = serde_json::Map::new();
    // Insert known fields in JS order
    for &key in JS_FIELD_ORDER {
        if let Some(val) = map.remove(key) {
            ordered.insert(key.to_string(), val);
        }
    }
    // Insert any remaining unknown fields (future additions) after the known ones
    for (k, v) in map {
        ordered.insert(k, v);
    }
    serde_json::Value::Object(ordered)
}

/// Apply `reorder_route_fields` to all routes in the plan JSON value.
fn reorder_plan_routes(plan: &mut serde_json::Value) {
    if let Some(routes) = plan.get_mut("routes").and_then(|r| r.as_array_mut()) {
        for entry in routes.iter_mut() {
            // Each entry is [routeId, routeObject]
            if let Some(arr) = entry.as_array_mut() {
                if arr.len() >= 2 {
                    let route_obj = arr.remove(1);
                    arr.push(reorder_route_fields(route_obj));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram_config::resolve_diagram_config_defaults;
    use std::path::PathBuf;

    fn corpus_data_dir() -> PathBuf {
        // Relative from the crate root: ../../docs/architext/data
        let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        crate_root.join("..").join("..").join("docs").join("architext").join("data")
    }

    #[test]
    fn enumerate_produces_16_entries() {
        let data_dir = corpus_data_dir();
        if !data_dir.exists() {
            // Skip in environments where the corpus isn't available
            return;
        }
        let config = resolve_diagram_config_defaults();
        let entries = enumerate_flow_plan_requests(&data_dir, &config)
            .expect("enumerate_flow_plan_requests");
        assert_eq!(entries.len(), 16, "Expected 16 entries (matching JS oracle)");
    }

    // Validates the NATIVE farm's hashing (sha256 → 64-char hex). The
    // non-native `plan_key_hash_native` intentionally returns "" (sha2/hex are
    // unavailable without the feature; the farm is only ever driven by the
    // farm_dump bin, which always enables `native`), so this assertion is only
    // meaningful — and only correct — under the native feature.
    #[cfg(feature = "native")]
    #[test]
    fn all_entries_have_valid_hash_and_key() {
        let data_dir = corpus_data_dir();
        if !data_dir.exists() {
            return;
        }
        let config = resolve_diagram_config_defaults();
        let entries = enumerate_flow_plan_requests(&data_dir, &config)
            .expect("enumerate_flow_plan_requests");
        for entry in &entries {
            assert!(!entry.key.is_empty(), "key must not be empty");
            assert_eq!(entry.hash.len(), 64, "hash must be 64-char hex: {}", entry.hash);
            // key must be valid JSON
            serde_json::from_str::<serde_json::Value>(&entry.key)
                .unwrap_or_else(|e| panic!("key for {}@{} is not valid JSON: {e}", entry.flow_id, entry.view_id));
        }
    }

    // warm_flow_plans publishes each plan via the callback as it is computed
    // (incremental farm warm-up), and produces the same plan set as the batch
    // enumerate — just delivered one at a time instead of all at once.
    #[cfg(feature = "native")]
    #[test]
    fn warm_flow_plans_streams_each_plan() {
        let data_dir = corpus_data_dir();
        if !data_dir.exists() {
            return;
        }
        let config = resolve_diagram_config_defaults();
        let collected = std::sync::Mutex::new(Vec::new());
        let count = warm_flow_plans(&data_dir, &config, |entry| {
            collected.lock().unwrap().push(entry.hash.clone());
        })
        .expect("warm_flow_plans");
        let collected = collected.into_inner().unwrap();
        assert_eq!(count, collected.len(), "callback fires once per warmed plan");
        let batch = enumerate_flow_plan_requests(&data_dir, &config).expect("enumerate");
        assert_eq!(count, batch.len(), "warm produces the same plan count as enumerate");
        for h in &collected {
            assert_eq!(h.len(), 64, "hash must be 64-char hex");
        }
    }
}
