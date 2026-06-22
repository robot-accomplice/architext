// Native routing fitness + perf ratchet over the sanitized high-complexity corpus.
//
// WHY: 1.7.0 moves routing from JS to Rust. The JS ratchet
// (test/routing-corpus-fitness.test.mjs) gates routing quality + work volume by
// running the JS engine over test/fixtures/routing-corpus/. When Phase 3 deletes
// the JS engine, that gate dies. This is the native equivalent: it runs the SAME
// corpus through the Rust `plan_diagram` path and freezes the gated quality
// metrics and deterministic work counters so no routing change can silently make
// a complex diagram worse — or do measurably more work — after the cutover.
//
// COVERAGE PARITY with the JS ratchet:
//   Quality (GATED_METRICS): routes, bends, crossings, pairInternalCrossings,
//     laneOrderViolations, closeParallelRuns, sharedSegments, repeatedCrossings.
//   Perf (PERF_GATED_COUNTERS): edgesPlanned, cheapCandidateCount, gridRouteCalls.
//   The JS Tier-2 machine-normalized wall-ratio is intentionally NOT ported: it is
//   non-deterministic and the task forbids gating wall-clock in CI. The deterministic
//   work counters above are the robust half and gate strictly with zero flake.
//
// Both gates judge ONE planning pass per flow (route_edges_with_stats), so quality
// and perf can never observe different plans.
//
// Re-baseline after an intentional, reviewed routing change:
//   REGEN_CORPUS_BASELINE=1 cargo test -p architext-routing --test corpus_fitness

use std::collections::BTreeMap;
use std::path::PathBuf;

use architext_routing::plan_request::diagram_layout::diagram_layout_for;
use architext_routing::plan_request::types::{View, ViewsFile};
use architext_routing::route_diagnostics::{
    diagnose_planned_routes, DiagOptions, DiagPlan, DiagRelationship,
};
use architext_routing::route_edges::{
    crossings_between, route_edges_with_stats, InputRelationship, NodeRect, RouteEdgesInput,
};
use architext_routing::model::Rect;
use indexmap::{IndexMap, IndexSet};
use serde::Deserialize;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Corpus input shapes (minimal — corpus steps carry only id/from/to[/kind/...])
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct CorpusStep {
    id: String,
    from: String,
    to: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, rename = "returnOf")]
    return_of: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CorpusFlow {
    id: String,
    steps: Vec<CorpusStep>,
}

#[derive(Debug, Deserialize)]
struct CorpusFlowsFile {
    flows: Vec<CorpusFlow>,
}

// ---------------------------------------------------------------------------
// Gated outputs (BTreeMap → stable key order for deterministic JSON snapshots)
// ---------------------------------------------------------------------------

/// The eight gated quality metrics, mirroring the JS GATED_METRICS set.
#[derive(Debug, Clone, PartialEq)]
struct FlowMetrics {
    routes: usize,
    bends: usize,
    crossings: usize,
    pair_internal_crossings: usize,
    lane_order_violations: usize,
    close_parallel_runs: usize,
    shared_segments: i64,
    repeated_crossings: i64,
    doglegs: usize,
    length: f64,
    bend_score: f64,
}

impl FlowMetrics {
    fn to_json(&self) -> Value {
        json!({
            "routes": self.routes,
            "bends": self.bends,
            "crossings": self.crossings,
            "pairInternalCrossings": self.pair_internal_crossings,
            "laneOrderViolations": self.lane_order_violations,
            "closeParallelRuns": self.close_parallel_runs,
            "sharedSegments": self.shared_segments,
            "repeatedCrossings": self.repeated_crossings,
            "doglegs": self.doglegs,
            "length": self.length,
            "bendScore": self.bend_score,
        })
    }
}

/// The three gated work counters, mirroring the JS PERF_GATED_COUNTERS set.
#[derive(Debug, Clone, PartialEq)]
struct FlowPerf {
    edges_planned: i64,
    cheap_candidate_count: i64,
    grid_route_calls: u64,
}

impl FlowPerf {
    fn to_json(&self) -> Value {
        json!({
            "edgesPlanned": self.edges_planned,
            "cheapCandidateCount": self.cheap_candidate_count,
            "gridRouteCalls": self.grid_route_calls,
        })
    }
}

// ---------------------------------------------------------------------------
// Corpus loading + the JS `renderedViewFor` projection rule
// ---------------------------------------------------------------------------

const FLOW_VIEW_TYPES: &[&str] = &["system-map", "flow-explorer", "workflow", "dataflow"];

fn corpus_dir() -> PathBuf {
    // tests run with CWD = crate dir; corpus lives at repo-root test/fixtures.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures/routing-corpus")
}

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus-fitness-baseline.json")
}

fn read_json(name: &str) -> Value {
    let raw = std::fs::read_to_string(corpus_dir().join(name))
        .unwrap_or_else(|e| panic!("read {name}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

fn flow_fits_view(flow: &CorpusFlow, view: &View) -> bool {
    if flow.steps.is_empty() {
        return false;
    }
    let ids: IndexSet<&str> = view
        .lanes
        .iter()
        .flat_map(|l| l.node_ids.iter().map(String::as_str))
        .collect();
    flow.steps
        .iter()
        .all(|s| ids.contains(s.from.as_str()) && ids.contains(s.to.as_str()))
}

/// Port of JS `renderedViewFor`: among flow-projection views the flow fits, the
/// first authored (non-system-map) one, else the first compatible one.
fn rendered_view_for<'a>(flow: &CorpusFlow, views: &'a [View]) -> Option<&'a View> {
    let compatible: Vec<&View> = views
        .iter()
        .filter(|v| FLOW_VIEW_TYPES.contains(&v.view_type.as_str()) && flow_fits_view(flow, v))
        .collect();
    compatible
        .iter()
        .find(|v| v.view_type != "system-map")
        .copied()
        .or_else(|| compatible.first().copied())
}

// ---------------------------------------------------------------------------
// One planning pass → (quality metrics, work counters), mirroring JS planCorpusFlow
// ---------------------------------------------------------------------------

fn plan_corpus_flow(flow: &CorpusFlow, views: &[View]) -> (FlowMetrics, FlowPerf) {
    let view = rendered_view_for(flow, views)
        .unwrap_or_else(|| panic!("No rendered view for corpus flow \"{}\"", flow.id));

    let visible_node_ids: Vec<String> = view
        .lanes
        .iter()
        .flat_map(|l| l.node_ids.iter().cloned())
        .collect();

    // Mirror JS: relationshipType "flow", displayIndex = index+1, kind/returnOf/outcome.
    let input_relationships: Vec<InputRelationship> = flow
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| InputRelationship {
            id: s.id.clone(),
            from: s.from.clone(),
            to: s.to.clone(),
            relationship_type: Some("flow".to_string()),
            kind: s.kind.clone(),
            return_of: s.return_of.clone(),
            outcome: s.outcome.clone(),
            step_id: None,
            flow_id: None,
            preferred_start_side: None,
            preferred_end_side: None,
            label: None,
            display_index: (i + 1) as i64,
        })
        .collect();

    let layout = diagram_layout_for(view, input_relationships.len(), None);

    // Build lane/row index maps + node rects exactly as plan_diagram does for
    // visible (in-view) nodes — the corpus has no extra (off-view) nodes.
    let mut lane_index_by_node: IndexMap<String, i64> = IndexMap::new();
    let mut row_index_by_node: IndexMap<String, i64> = IndexMap::new();
    let visible_set: IndexSet<String> = visible_node_ids.iter().cloned().collect();
    for (lane_index, lane) in view.lanes.iter().enumerate() {
        for (row_index, node_id) in lane.node_ids.iter().enumerate() {
            if !visible_set.contains(node_id) {
                continue;
            }
            lane_index_by_node.insert(node_id.clone(), lane_index as i64);
            row_index_by_node.insert(node_id.clone(), row_index as i64);
        }
    }

    let max_rows: i64 = view
        .lanes
        .iter()
        .map(|l| l.node_ids.iter().filter(|id| visible_set.contains(*id)).count() as i64)
        .max()
        .unwrap_or(0)
        .max(1);
    let canvas_width = f64::max(
        layout.min_canvas_width,
        layout.margin_x * 2.0 + view.lanes.len() as f64 * layout.lane_width + layout.canvas_extra_width,
    );
    let canvas_height = f64::max(
        layout.min_canvas_height,
        layout.margin_y + max_rows as f64 * layout.row_gap + layout.canvas_extra_height,
    );

    let mut node_rects: IndexMap<String, Rect> = IndexMap::new();
    let mut route_node_rects: IndexMap<String, NodeRect> = IndexMap::new();
    for node_id in &visible_node_ids {
        let lane_idx = lane_index_by_node.get(node_id).copied().unwrap_or(0);
        let row_idx = row_index_by_node.get(node_id).copied().unwrap_or(0);
        let rect = Rect {
            x: layout.margin_x + lane_idx as f64 * layout.lane_width,
            y: layout.margin_y + row_idx as f64 * layout.row_gap,
            width: layout.node_width,
            height: layout.node_height,
        };
        node_rects.insert(node_id.clone(), rect.clone());
        route_node_rects.insert(
            node_id.clone(),
            NodeRect { rect, fixed_ports: false, side_anchors: None },
        );
    }

    let route_input = RouteEdgesInput {
        style: "orthogonal".to_string(),
        relationships: input_relationships.clone(),
        visible_node_ids: visible_node_ids.clone(),
        node_rects: route_node_rects,
        lane_index_by_node: lane_index_by_node.clone(),
        row_index_by_node: row_index_by_node.clone(),
        canvas_width,
        canvas_height,
        margin_y: layout.margin_y,
        grid_route_max_points: 600,
        grid_route_max_expansions: 3000,
        score_edge_proximity: false,
        // This frozen-baseline gate guards the legacy ENGINE path (it calls
        // route_edges_with_stats directly, not the model's plan_diagram overwrite).
        // Force the engine so the baseline stays meaningful now that the model is
        // the production default; the model has its own gate (audit_model corpus test).
        force_engine: true,
    };

    let (routes, stats) = route_edges_with_stats(&route_input);

    // Aggregate crossings over every route pair (JS totalCrossings).
    let all: Vec<_> = routes.values().collect();
    let mut total_crossings = 0usize;
    for a in 0..all.len() {
        for b in (a + 1)..all.len() {
            total_crossings += crossings_between(all[a], all[b]);
        }
    }

    // Diagnostics over the planned RouteData (the same objects route_edges produced).
    let diag_relationships: Vec<DiagRelationship> = input_relationships
        .iter()
        .map(|r| DiagRelationship {
            id: r.id.clone(),
            from: r.from.clone(),
            to: r.to.clone(),
            display_index: Some(r.display_index as f64),
            kind: r.kind.clone(),
            return_of: r.return_of.clone(),
            outcome: r.outcome.clone(),
            relationship_type: r.relationship_type.clone(),
            step_id: r.step_id.clone(),
            flow_id: r.flow_id.clone(),
            preferred_start_side: r.preferred_start_side.clone(),
            preferred_end_side: r.preferred_end_side.clone(),
        })
        .collect();

    let diag_plan = DiagPlan {
        routes: &routes,
        node_rects: &node_rects,
        visible_node_ids: &visible_set,
        lane_index_by_node: &lane_index_by_node,
        row_index_by_node: &row_index_by_node,
        canvas_width,
        canvas_height,
    };
    let diag = diagnose_planned_routes(&diag_plan, &diag_relationships, &DiagOptions::default());

    let metrics = FlowMetrics {
        routes: diag.metrics.routes,
        bends: diag.metrics.bends,
        crossings: total_crossings,
        doglegs: diag.metrics.doglegs,
        length: diag.metrics.total_length,
        bend_score: diag.metrics.bend_score,
        pair_internal_crossings: diag.metrics.pair_internal_crossings,
        lane_order_violations: diag.metrics.lane_order_violations,
        close_parallel_runs: diag.metrics.close_parallel_runs,
        shared_segments: diag.metrics.shared_segments,
        repeated_crossings: diag.metrics.repeated_crossings,
    };
    let perf = FlowPerf {
        edges_planned: stats.edges_planned,
        cheap_candidate_count: stats.cheap_candidate_count,
        grid_route_calls: stats.grid_route_calls,
    };
    (metrics, perf)
}

fn compute_corpus() -> (BTreeMap<String, FlowMetrics>, BTreeMap<String, FlowPerf>) {
    let views: Vec<View> = serde_json::from_value::<ViewsFile>(read_json("views.json"))
        .expect("views.json")
        .views;
    let flows: Vec<CorpusFlow> = serde_json::from_value::<CorpusFlowsFile>(read_json("flows.json"))
        .expect("flows.json")
        .flows;

    let mut metrics = BTreeMap::new();
    let mut perf = BTreeMap::new();
    for flow in &flows {
        let (m, p) = plan_corpus_flow(flow, &views);
        metrics.insert(flow.id.clone(), m);
        perf.insert(flow.id.clone(), p);
    }
    (metrics, perf)
}

fn snapshot_json(
    metrics: &BTreeMap<String, FlowMetrics>,
    perf: &BTreeMap<String, FlowPerf>,
) -> Value {
    let m: serde_json::Map<String, Value> =
        metrics.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
    let p: serde_json::Map<String, Value> =
        perf.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
    json!({ "metrics": m, "perf": p })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn corpus_fitness_and_perf_hold_the_frozen_baseline() {
    let (metrics, perf) = compute_corpus();
    let current = snapshot_json(&metrics, &perf);

    let path = baseline_path();
    if std::env::var("REGEN_CORPUS_BASELINE").is_ok() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&current).unwrap()))
            .unwrap();
        eprintln!(
            "WROTE Rust corpus baseline → {} (review the values before committing)",
            path.display()
        );
        return;
    }

    let baseline: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read baseline")).unwrap();
    let base_metrics = baseline["metrics"].as_object().expect("baseline.metrics");
    let base_perf = baseline["perf"].as_object().expect("baseline.perf");

    // Flow set must be identical.
    let cur_ids: Vec<&String> = metrics.keys().collect();
    let base_ids: Vec<&String> = base_metrics.keys().collect();
    assert_eq!(
        cur_ids, base_ids,
        "corpus flow set changed; re-run with REGEN_CORPUS_BASELINE=1 if intentional"
    );

    // --- Quality gate: every gated metric must equal the frozen baseline. ---
    let mut quality_drift = Vec::new();
    for (flow, m) in &metrics {
        let was = &base_metrics[flow];
        let now = m.to_json();
        for key in [
            "routes",
            "bends",
            "crossings",
            "doglegs",
            "pairInternalCrossings",
            "laneOrderViolations",
            "closeParallelRuns",
            "sharedSegments",
            "repeatedCrossings",
        ] {
            let w = was[key].as_i64().unwrap();
            let n = now[key].as_i64().unwrap();
            if n != w {
                let dir = if n > w { "REGRESSED" } else { "improved" };
                quality_drift.push(format!("{flow}.{key}: {w} -> {n} ({dir})"));
            }
        }
    }
    assert!(
        quality_drift.is_empty(),
        "routing quality metrics drifted from baseline:\n  {}\n\
         If this is an intentional, reviewed routing change, regenerate the baseline:\n  \
         REGEN_CORPUS_BASELINE=1 cargo test -p architext-routing --test corpus_fitness",
        quality_drift.join("\n  ")
    );

    // --- Perf gate: work counters must not exceed the frozen baseline. ---
    // Counters are exact for fixed inputs on any machine; any increase is a real
    // volume regression, never flake. The bar only moves toward improvement.
    let mut perf_drift = Vec::new();
    for (flow, p) in &perf {
        let was = &base_perf[flow];
        let now = p.to_json();
        for key in ["edgesPlanned", "cheapCandidateCount", "gridRouteCalls"] {
            let w = was[key].as_i64().unwrap();
            let n = now[key].as_i64().unwrap();
            if n > w {
                perf_drift.push(format!("{flow}.{key}: {w} -> {n} (REGRESSED)"));
            }
        }
    }
    assert!(
        perf_drift.is_empty(),
        "planner work volume regressed beyond the perf baseline:\n  {}\n\
         The perf bar only moves toward improvement. Fix the regression; if work \
         legitimately decreased and you want to tighten the bar, run:\n  \
         REGEN_CORPUS_BASELINE=1 cargo test -p architext-routing --test corpus_fitness",
        perf_drift.join("\n  ")
    );
}
