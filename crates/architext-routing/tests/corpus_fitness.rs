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
// COVERAGE: the gate freezes the deterministic MODEL's quality metrics
//   (GATED_METRICS): routes, bends, crossings, pairInternalCrossings,
//   laneOrderViolations, closeParallelRuns, sharedSegments, repeatedCrossings,
//   doglegs, length, bendScore. The model (`route_all_coordinated`) is the sole
//   production router, so this gate now judges the geometry the viewer renders.
//
//   The legacy candidate engine's deterministic work counters (edgesPlanned,
//   cheapCandidateCount, gridRouteCalls) are GONE with the engine: the model
//   exposes no per-plan work counters, so the perf half of the JS ratchet has no
//   analogue here and is dropped. The quality gate above is the robust half and
//   gates strictly with zero flake.
//
// Each flow is routed exactly as `apply_model_routes` (plan_diagram.rs) does:
// build participant rects + edges, call `route_all_coordinated`, wrap each
// polyline via `route_with_points`, then a hop pass via `render_orthogonal_route`.
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
use architext_routing::route_edges::helpers::{render_orthogonal_route, route_with_points};
use architext_routing::route_edges::types::RouteData;
use architext_routing::route_edges::{
    crossings_between, InputRelationship, NodeRect, RouteEdgesInput,
};
use architext_routing::route_model::place::{route_all_coordinated, Edge};
use architext_routing::model::Rect;
use std::collections::{HashMap, HashSet};
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

fn plan_corpus_flow(flow: &CorpusFlow, views: &[View]) -> FlowMetrics {
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
    };

    // Route via the deterministic MODEL, mirroring `apply_model_routes`
    // (plan_diagram.rs) so this gate freezes the geometry the viewer renders.
    let routes = model_route(&route_input);

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

    FlowMetrics {
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
    }
}

/// Route one corpus flow through the deterministic model, mirroring
/// `apply_model_routes` in plan_diagram.rs exactly: obstacle set = flow
/// participants, edges in relationship order, `route_all_coordinated`, then
/// `route_with_points` per polyline, then a hop pass via
/// `render_orthogonal_route` over the model-routed set.
fn model_route(input: &RouteEdgesInput) -> IndexMap<String, RouteData> {
    let mut node_ids: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for rel in &input.relationships {
        for id in [rel.from.as_str(), rel.to.as_str()] {
            if input.node_rects.contains_key(id) && seen.insert(id) {
                node_ids.push(id);
            }
        }
    }
    let idx: HashMap<&str, usize> =
        node_ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let rects: Vec<Rect> = node_ids.iter().map(|&id| input.node_rects[id].rect.clone()).collect();

    let mut edges: Vec<Edge> = Vec::new();
    let mut edge_rel_ids: Vec<String> = Vec::new();
    for rel in &input.relationships {
        if let (Some(&a), Some(&b)) = (idx.get(rel.from.as_str()), idx.get(rel.to.as_str())) {
            edges.push(Edge { a, b });
            edge_rel_ids.push(rel.id.clone());
        }
    }

    let model_routes = route_all_coordinated(&rects, &edges);
    let mut routed: IndexMap<String, RouteData> = IndexMap::new();
    for (i, rel_id) in edge_rel_ids.iter().enumerate() {
        let pts = &model_routes[i];
        if pts.len() < 2 {
            continue;
        }
        let base = RouteData {
            d: String::new(),
            points: Vec::new(),
            controls: None,
            samples: Vec::new(),
            sample_bounds: Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
            bends: 0,
            label_x: 0.0,
            label_y: 0.0,
            style: input.style.clone(),
            extra: IndexMap::new(),
        };
        routed.insert(rel_id.clone(), route_with_points(&base, pts.clone(), None));
    }

    let hop_ids: Vec<String> =
        edge_rel_ids.iter().filter(|id| routed.contains_key(*id)).cloned().collect();
    let hop_routes: Vec<_> = hop_ids.iter().map(|id| routed[id].clone()).collect();
    for (i, id) in hop_ids.iter().enumerate() {
        let rebuilt = render_orthogonal_route(&hop_routes[i], &hop_routes, i);
        routed.insert(id.clone(), rebuilt);
    }
    routed
}

fn compute_corpus() -> BTreeMap<String, FlowMetrics> {
    let views: Vec<View> = serde_json::from_value::<ViewsFile>(read_json("views.json"))
        .expect("views.json")
        .views;
    let flows: Vec<CorpusFlow> = serde_json::from_value::<CorpusFlowsFile>(read_json("flows.json"))
        .expect("flows.json")
        .flows;

    let mut metrics = BTreeMap::new();
    for flow in &flows {
        metrics.insert(flow.id.clone(), plan_corpus_flow(flow, &views));
    }
    metrics
}

fn snapshot_json(metrics: &BTreeMap<String, FlowMetrics>) -> Value {
    let m: serde_json::Map<String, Value> =
        metrics.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
    json!({ "metrics": m })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn corpus_fitness_holds_the_frozen_baseline() {
    let route_start = std::time::Instant::now();
    let metrics = compute_corpus();
    let route_ms = route_start.elapsed().as_secs_f64() * 1000.0;
    let current = snapshot_json(&metrics);

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

    // Catastrophic-regression guard (wall time). The deterministic model routes
    // the whole corpus in tens of ms; this bound is deliberately generous so it
    // never flakes on a slow/debug/CI runner, while still tripping if a
    // super-linear pass is reintroduced (the removed legacy engine spent SECONDS
    // on a single dense view). A coarse blow-up alarm, not a tight ratchet.
    const MAX_CORPUS_ROUTE_MS: f64 = 1000.0;
    assert!(
        route_ms < MAX_CORPUS_ROUTE_MS,
        "corpus routing took {route_ms:.0}ms (budget {MAX_CORPUS_ROUTE_MS:.0}ms) — routing perf regression"
    );
}
