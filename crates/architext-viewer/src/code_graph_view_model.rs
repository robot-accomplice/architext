//! Pure view-model for the WebGL2 code-graph view (Plan C Task 4).
//!
//! Zero Leptos, zero web-sys — everything here is unit-testable natively.
//! The Leptos surface (`components/code_graph_view.rs`) owns signals, the
//! render loop, and the chrome; this module owns the facts those render:
//!
//! - [`build_graph`] — `CodeGraph` (the Magma document) → [`GraphModel`]
//!   (labels, radii, reachability flags, directed edges, adjacency) for one
//!   tier. Lifted from the spike's `build_graph`.
//! - [`cull`] — the REQUIRED behaviour change from the spike: filters CULL.
//!   Excluded nodes/edges are never uploaded to the GPU, so a filter change
//!   recomputes the upload sets here and the view re-uploads, instead of the
//!   spike's draw-culled-items-at-low-alpha fade (a measured haze at scale).
//! - [`node_state`] / [`edge_state`] — per-instance `[alpha, glow, colorMix,
//!   0]` / `[alpha, colorMix, 0, 0]` interleaves, exactly the dynamic-buffer
//!   contract documented in `gl/shaders.rs` and `gl/renderer.rs`.
//! - [`ViewState`] — the imperative per-frame state (camera, selection,
//!   filter, animation) the view keeps OUTSIDE Leptos signals (a redraw is
//!   GPU submission work, not a vdom diff — same rationale as the spike).
//!
//! Traversal/filter primitives are Task 2's (`code_graph_graph::GraphIndex`,
//! `bfs`, `FilterState`) — used, never reimplemented.
use crate::code_graph_graph::{Direction, FilterState, GraphIndex};
use crate::data::models::CodeGraph;
use crate::force_layout::QuadTree;

// Un-reached nodes during animation must "recede hard" (spike brief).
const ANIM_FADE_ALPHA: f32 = 0.025;
const TRAIL_ALPHA: f32 = 0.35;
const SELECT_FADE_ALPHA: f32 = 0.05;

/// The two granularities the view renders. Functions is the default tier
/// (the one that auto-plays the roots animation on open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Modules,
    Functions,
}

/// Call-order animation wavefront: `Roots` sweeps from the entrypoints,
/// `Outbound`/`Inbound` sweep from the selected node (callees / callers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimMode {
    Off,
    Roots,
    Outbound,
    Inbound,
}

/// One tier's graph, built once per (document, tier) pair. All vecs are
/// index-aligned with the node list; edges are index pairs (never string
/// ids — ids are mapped to indices once here so everything downstream stays
/// cheap at 17,561 nodes / 49,368 edges).
pub struct GraphModel {
    pub labels: Vec<String>,
    pub degree: Vec<u32>,
    pub radius: Vec<f32>,
    // The four reachability-class slices `FilterState::visible_nodes`
    // consumes, in that order.
    pub prod_reachable: Vec<bool>,
    pub dead: Vec<bool>,
    pub test_only: Vec<bool>,
    pub generated: Vec<bool>,
    /// Entrypoint indices (function tier only) — the `Roots` animation seeds.
    pub roots: Vec<usize>,
    /// Undirected pairs, deduped — fed to the force simulation only.
    pub layout_edges: Vec<(usize, usize)>,
    /// Directed (from, to, is_dynamic) — drives the animation, the edge-kind
    /// filter, and the edge uploads.
    pub directed_edges: Vec<(usize, usize, bool)>,
    /// Undirected adjacency for selection highlight (neighbours light up).
    pub neighbors: Vec<Vec<usize>>,
    /// Directed adjacency for BFS wavefronts (Task 2's index).
    pub index: GraphIndex,
}

impl GraphModel {
    pub fn node_count(&self) -> usize {
        self.labels.len()
    }
}

/// Node radius keys off total degree (fan-in + fan-out), clamped so hubs
/// dominate without swallowing the canvas. Same formula as the spike.
fn radius_for(degree: u32) -> f32 {
    (3.0 + (degree as f32).sqrt() * 1.7).clamp(3.0, 22.0)
}

/// Build one tier's [`GraphModel`] from the Magma document.
pub fn build_graph(cg: &CodeGraph, tier: Tier) -> GraphModel {
    let mut labels = Vec::new();
    let mut degree = Vec::new();
    let mut prod_reachable = Vec::new();
    let mut dead = Vec::new();
    let mut test_only = Vec::new();
    let mut generated = Vec::new();
    let mut roots = Vec::new();
    let mut directed_edges: Vec<(usize, usize, bool)> = Vec::new();

    match tier {
        Tier::Modules => {
            let empty_m = Vec::new();
            let empty_c = Vec::new();
            let modules = cg.modules.as_ref().unwrap_or(&empty_m);
            let calls = cg.module_calls.as_ref().unwrap_or(&empty_c);
            let index: std::collections::HashMap<&str, usize> =
                modules.iter().enumerate().map(|(i, m)| (m.id.as_str(), i)).collect();
            for m in modules {
                labels.push(m.pkg.clone());
                degree.push(m.fan_in + m.fan_out);
                // Module granularity has no per-module prod-reachability flag,
                // so every module counts as prod-reachable and the default
                // (prod-only) filter shows the whole tier — the reachability
                // filter only has bite at the function tier (spike behaviour).
                prod_reachable.push(true);
                dead.push(m.counts.dead > 0 && m.counts.dead == m.counts.functions);
                test_only.push(m.counts.test_only > 0 && m.counts.test_only == m.counts.functions);
                generated.push(false);
            }
            for c in calls {
                if let (Some(&a), Some(&b)) = (index.get(c.from.as_str()), index.get(c.to.as_str())) {
                    if a != b {
                        directed_edges.push((a, b, c.has_dynamic));
                    }
                }
            }
        }
        Tier::Functions => {
            let empty_f = Vec::new();
            let empty_c = Vec::new();
            let functions = cg.functions.as_ref().unwrap_or(&empty_f);
            let calls = cg.calls.as_ref().unwrap_or(&empty_c);
            let index: std::collections::HashMap<&str, usize> =
                functions.iter().enumerate().map(|(i, f)| (f.id.as_str(), i)).collect();
            for (i, f) in functions.iter().enumerate() {
                labels.push(f.symbol.clone());
                degree.push(f.fan_in + f.fan_out);
                prod_reachable.push(f.prod_reachable);
                dead.push(!f.reachable);
                test_only.push(f.test);
                generated.push(f.generated);
                if f.root {
                    roots.push(i);
                }
            }
            for c in calls {
                if let (Some(&a), Some(&b)) = (index.get(c.from.as_str()), index.get(c.to.as_str())) {
                    if a != b {
                        directed_edges.push((a, b, c.kind == "dynamic"));
                    }
                }
            }
        }
    }

    let radius = degree.iter().map(|&d| radius_for(d)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut layout_edges = Vec::new();
    let mut neighbors = vec![Vec::new(); labels.len()];
    for &(a, b, _) in &directed_edges {
        neighbors[a].push(b);
        neighbors[b].push(a);
        let key = (a.min(b), a.max(b));
        if seen.insert(key) {
            layout_edges.push(key);
        }
    }
    let index = GraphIndex::from_edges(
        labels.len(),
        &directed_edges.iter().map(|&(a, b, _)| (a, b)).collect::<Vec<_>>(),
    );

    GraphModel {
        labels,
        degree,
        radius,
        prod_reachable,
        dead,
        test_only,
        generated,
        roots,
        layout_edges,
        directed_edges,
        neighbors,
        index,
    }
}

/// The result of applying a [`FilterState`] to a graph: WHAT GETS UPLOADED.
/// `nodes`/`edges` hold FULL-graph indices; `visible` is the full-graph node
/// lookup (hit-testing must not select a culled node through the full-graph
/// quadtree, and a selection culled by a filter change is cleared).
pub struct Cull {
    pub visible: Vec<bool>,
    pub nodes: Vec<usize>,
    pub edges: Vec<(usize, usize, bool)>,
}

/// Compute the cull sets. A node survives if the filter shows its class; an
/// edge survives if BOTH endpoints survive and the filter shows its kind.
pub fn cull(graph: &GraphModel, filter: &FilterState) -> Cull {
    let visible = filter.visible_nodes(
        &graph.prod_reachable,
        &graph.dead,
        &graph.test_only,
        &graph.generated,
    );
    let nodes: Vec<usize> = (0..graph.node_count()).filter(|&i| visible[i]).collect();
    let edges: Vec<(usize, usize, bool)> = graph
        .directed_edges
        .iter()
        .copied()
        .filter(|&(a, b, dynamic)| visible[a] && visible[b] && filter.edge_visible(dynamic))
        .collect();
    Cull { visible, nodes, edges }
}

/// BFS layers → per-node depth field (`-1` = unreached), the shape the
/// animation state computation indexes by node.
pub fn depth_field(node_count: usize, layers: &[Vec<usize>]) -> Vec<i32> {
    let mut depth = vec![-1i32; node_count];
    for (d, layer) in layers.iter().enumerate() {
        for &i in layer {
            depth[i] = d as i32;
        }
    }
    depth
}

/// The STATIC interleaves for `Renderer::upload_static`, over the CULLED
/// sets: `[x, y, radius] * visible_nodes` and `[from_x, from_y, to_x, to_y]
/// * visible_edges`. `positions` is the full-graph layout (stable across
///   filter changes — culling never re-runs the simulation).
pub fn static_interleaves(
    graph: &GraphModel,
    c: &Cull,
    positions: &[(f32, f32)],
) -> (Vec<f32>, Vec<f32>) {
    let mut node_pos_radius = Vec::with_capacity(c.nodes.len() * 3);
    for &i in &c.nodes {
        let (x, y) = positions[i];
        node_pos_radius.extend_from_slice(&[x, y, graph.radius[i]]);
    }
    let mut edge_endpoints = Vec::with_capacity(c.edges.len() * 4);
    for &(a, b, _) in &c.edges {
        let (ax, ay) = positions[a];
        let (bx, by) = positions[b];
        edge_endpoints.extend_from_slice(&[ax, ay, bx, by]);
    }
    (node_pos_radius, edge_endpoints)
}

/// Per-uploaded-node `[alpha, glow, colorMix, 0]` — folds the animation
/// wavefront and the selection highlight into one pass so they compose
/// instead of fighting. (No filter fold: culled nodes are not uploaded at
/// all — that is the whole point of the cull.)
pub fn node_state(
    graph: &GraphModel,
    c: &Cull,
    selected: Option<usize>,
    anim_mode: AnimMode,
    anim_depth: &[i32],
    anim_current: i32,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(c.nodes.len() * 4);
    for &g in &c.nodes {
        let (mut alpha, mut glow, mut mix) = (0.85_f32, 0.0_f32, 0.0_f32);
        if anim_mode != AnimMode::Off {
            let d = anim_depth[g];
            if d < 0 || d > anim_current {
                alpha = ANIM_FADE_ALPHA;
            } else if d == anim_current {
                alpha = 1.0;
                glow = 0.9;
                mix = 1.0;
            } else {
                alpha = TRAIL_ALPHA;
                mix = 0.6;
            }
        } else if let Some(s) = selected {
            if g == s {
                alpha = 1.0;
                glow = 0.8;
                mix = 1.0;
            } else if graph.neighbors[s].contains(&g) {
                alpha = 1.0;
                mix = 1.0;
            } else {
                alpha = SELECT_FADE_ALPHA;
            }
        }
        out.extend_from_slice(&[alpha, glow, mix, 0.0]);
    }
    out
}

/// Per-uploaded-edge `[alpha, colorMix, 0, 0]`. The base alpha drops at
/// scale (>4000 visible edges) so a dense tier stays readable — same
/// threshold as the spike.
pub fn edge_state(
    c: &Cull,
    selected: Option<usize>,
    anim_mode: AnimMode,
    anim_depth: &[i32],
    anim_current: i32,
) -> Vec<f32> {
    let base = if c.edges.len() > 4000 { 0.05 } else { 0.28 };
    let mut out = Vec::with_capacity(c.edges.len() * 4);
    for &(a, b, _) in &c.edges {
        let (mut alpha, mut mix) = (base, 0.0_f32);
        if anim_mode != AnimMode::Off {
            let (da, db) = (anim_depth[a], anim_depth[b]);
            let both_reached = da >= 0 && db >= 0 && da <= anim_current && db <= anim_current;
            if !both_reached {
                alpha = ANIM_FADE_ALPHA;
            } else if da == anim_current || db == anim_current {
                alpha = 0.9;
                mix = 1.0;
            } else {
                alpha = 0.22;
                mix = 0.5;
            }
        } else if let Some(s) = selected {
            if a == s || b == s {
                alpha = 0.9;
                mix = 1.0;
            } else {
                alpha = SELECT_FADE_ALPHA * 0.5;
            }
        }
        out.extend_from_slice(&[alpha, mix, 0.0, 0.0]);
    }
    out
}

/// Fit-to-viewport zoom for a fresh layout, against the 90th-percentile
/// extent (a handful of low-degree outliers sit at the gravity equilibrium
/// ring and must not dictate the zoom alone — spike-proven framing).
pub fn fit_zoom(positions: &[(f32, f32)], w: f32, h: f32) -> f32 {
    let fit_bound = |mut vals: Vec<f32>| -> f32 {
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((vals.len() as f32 - 1.0) * 0.90).round().max(0.0) as usize;
        vals.get(idx).copied().unwrap_or(1.0).max(1.0)
    };
    let max_x = fit_bound(positions.iter().map(|(x, _)| x.abs()).collect());
    let max_y = fit_bound(positions.iter().map(|(_, y)| y.abs()).collect());
    (w / (max_x * 2.2)).min(h / (max_y * 2.2)).clamp(0.02, 3.0)
}

/// The imperative per-frame state. Kept out of Leptos signals by the view —
/// a redraw is GPU submission work, not a vdom diff; signals only MIRROR the
/// few display facts (counts, selected label, depth) for the chrome.
pub struct ViewState {
    pub graph: GraphModel,
    /// Full-graph layout positions (stable across filter changes).
    pub positions: Vec<(f32, f32)>,
    /// Hit-test tree over the FULL graph (culled hits are rejected via
    /// `cull.visible`, keeping the tree — and the layout — filter-stable).
    pub tree: QuadTree,
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    /// Selected node (FULL-graph index). Panel-local by construction — this
    /// is never written to `AppState::selected_node` (different id-space).
    pub selected: Option<usize>,
    pub filter: FilterState,
    pub cull: Cull,
    pub anim_mode: AnimMode,
    /// Per FULL-graph node, `-1` = unreached (indexes the whole graph so the
    /// wavefront composes with culling: culled nodes simply aren't drawn).
    pub anim_depth: Vec<i32>,
    pub anim_max_depth: i32,
    pub anim_current_depth: i32,
}

impl ViewState {
    /// Fresh state for a newly built tier: default filter (prod-reachable
    /// only — filters are ON by default), cull applied, animation off.
    pub fn new(
        graph: GraphModel,
        positions: Vec<(f32, f32)>,
        tree: QuadTree,
        pan_x: f32,
        pan_y: f32,
        zoom: f32,
    ) -> Self {
        let filter = FilterState::default();
        let cull = cull(&graph, &filter);
        Self {
            graph,
            positions,
            tree,
            pan_x,
            pan_y,
            zoom,
            selected: None,
            filter,
            cull,
            anim_mode: AnimMode::Off,
            anim_depth: Vec::new(),
            anim_max_depth: 0,
            anim_current_depth: 0,
        }
    }

    /// Re-apply the current filter (after a checkbox flip). A selection
    /// culled by the new filter is cleared — it is no longer on the canvas.
    pub fn recompute_cull(&mut self) {
        self.cull = cull(&self.graph, &self.filter);
        if let Some(s) = self.selected {
            if !self.cull.visible[s] {
                self.selected = None;
            }
        }
    }

    /// Re-run the BFS wavefront for the current animation mode (seeds: the
    /// roots, or the selected node for Outbound/Inbound).
    pub fn recompute_bfs(&mut self) {
        let n = self.graph.node_count();
        let layers = match self.anim_mode {
            AnimMode::Off => Vec::new(),
            AnimMode::Roots => self.graph.index.bfs(Direction::Outbound, &self.graph.roots),
            AnimMode::Outbound => self
                .selected
                .map(|s| self.graph.index.bfs(Direction::Outbound, &[s]))
                .unwrap_or_default(),
            AnimMode::Inbound => self
                .selected
                .map(|s| self.graph.index.bfs(Direction::Inbound, &[s]))
                .unwrap_or_default(),
        };
        // BFS layers are contiguous, so the layer count minus one IS the max
        // depth (the spike derived it from the depth field — same value).
        self.anim_max_depth = layers.len().saturating_sub(1) as i32;
        self.anim_depth = depth_field(n, &layers);
        self.anim_current_depth = 0;
    }

    pub fn static_interleaves(&self) -> (Vec<f32>, Vec<f32>) {
        static_interleaves(&self.graph, &self.cull, &self.positions)
    }

    pub fn node_state(&self) -> Vec<f32> {
        node_state(
            &self.graph,
            &self.cull,
            self.selected,
            self.anim_mode,
            &self.anim_depth,
            self.anim_current_depth,
        )
    }

    pub fn edge_state(&self) -> Vec<f32> {
        edge_state(
            &self.cull,
            self.selected,
            self.anim_mode,
            &self.anim_depth,
            self.anim_current_depth,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::force_layout::{simulate, ForceConfig};

    /// A 4-function graph: root `main` (prod) → `handler` (prod) → `helper`
    /// (dead), plus `tests` (a test function). One static + one dynamic call.
    fn fixture() -> CodeGraph {
        serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1", "generator": "magma",
            "language": "go", "module": "example.com/x", "sha": "a",
            "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [
                {"id": "main", "symbol": "main.main", "pkg": "main", "file": "m.go", "line": 1,
                 "kind": "func", "exported": false, "test": false, "root": true,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 1},
                {"id": "handler", "symbol": "srv.handle", "pkg": "srv", "file": "h.go", "line": 2,
                 "kind": "func", "exported": true, "test": false, "root": false,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 1, "fan_out": 2},
                {"id": "helper", "symbol": "srv.help", "pkg": "srv", "file": "h.go", "line": 9,
                 "kind": "func", "exported": false, "test": false, "root": false,
                 "generated": false, "reachable": false, "prod_reachable": false,
                 "signature": {"params": [], "results": []}, "fan_in": 1, "fan_out": 0},
                {"id": "tests", "symbol": "srv.TestHandle", "pkg": "srv", "file": "h_test.go", "line": 1,
                 "kind": "func", "exported": true, "test": true, "root": true,
                 "generated": false, "reachable": true, "prod_reachable": false,
                 "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 1}
            ],
            "calls": [
                {"from": "main", "to": "handler", "site_file": "m.go", "site_line": 3, "kind": "static"},
                {"from": "handler", "to": "helper", "site_file": "h.go", "site_line": 4, "kind": "dynamic"},
                {"from": "tests", "to": "handler", "site_file": "h_test.go", "site_line": 5, "kind": "static"}
            ]
        }))
        .expect("fixture parses")
    }

    #[test]
    fn build_graph_maps_functions_flags_edges_and_roots() {
        let g = build_graph(&fixture(), Tier::Functions);
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.labels[1], "srv.handle");
        assert_eq!(g.prod_reachable, vec![true, true, false, false]);
        assert_eq!(g.dead, vec![false, false, true, false]);
        assert_eq!(g.test_only, vec![false, false, false, true]);
        assert_eq!(g.roots, vec![0, 3], "main and the test are roots");
        assert_eq!(g.directed_edges.len(), 3);
        assert!(g.directed_edges[1].2, "handler→helper is dynamic");
        // Layout edges are undirected + deduped; neighbours are bidirectional.
        assert_eq!(g.layout_edges.len(), 3);
        assert!(g.neighbors[1].contains(&0) && g.neighbors[1].contains(&2));
    }

    #[test]
    fn default_filter_culls_to_prod_reachable_and_drops_their_edges() {
        // THE required behaviour change: culling REMOVES, it does not fade.
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default());
        assert_eq!(c.nodes, vec![0, 1], "dead + test nodes are culled by default");
        assert_eq!(c.edges.len(), 1, "edges touching culled nodes are culled too");
        assert_eq!(c.edges[0], (0, 1, false), "only main→handler survives");
    }

    #[test]
    fn edge_kind_filter_culls_dynamic_edges() {
        let g = build_graph(&fixture(), Tier::Functions);
        let mut f = FilterState {
            show_prod_reachable: true,
            show_dead: true,
            show_test_only: true,
            show_generated: true,
            show_static: true,
            show_dynamic: true,
        };
        assert_eq!(cull(&g, &f).edges.len(), 3, "everything shown");
        f.show_dynamic = false;
        let c = cull(&g, &f);
        assert_eq!(c.edges.len(), 2, "the dynamic edge is culled");
        assert!(c.edges.iter().all(|&(_, _, d)| !d));
    }

    #[test]
    fn depth_field_marks_unreached_minus_one() {
        let g = build_graph(&fixture(), Tier::Functions);
        let layers = g.index.bfs(Direction::Outbound, &[0]);
        let depth = depth_field(4, &layers);
        assert_eq!(depth[0], 0);
        assert_eq!(depth[1], 1);
        assert_eq!(depth[2], 2, "main→handler→helper");
        assert_eq!(depth[3], -1, "the test root is unreached from main");
    }

    #[test]
    fn node_state_interleave_matches_the_gpu_contract() {
        // 4 f32 per uploaded node: [alpha, glow, colorMix, 0].
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default());
        let s = node_state(&g, &c, None, AnimMode::Off, &[], 0);
        assert_eq!(s.len(), c.nodes.len() * 4);
        assert_eq!(&s[0..4], &[0.85, 0.0, 0.0, 0.0], "base state, no selection/anim");
    }

    #[test]
    fn node_state_selection_highlights_self_and_neighbours() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default());
        // Select handler (graph idx 1); its uploaded neighbour is main (0).
        let s = node_state(&g, &c, Some(1), AnimMode::Off, &[], 0);
        let slot_main = &s[0..4];
        let slot_handler = &s[4..8];
        assert_eq!(slot_handler, &[1.0, 0.8, 1.0, 0.0], "selected: full alpha + glow");
        assert_eq!(slot_main, &[1.0, 0.0, 1.0, 0.0], "neighbour: full alpha, no glow");
    }

    #[test]
    fn node_state_animation_wavefront_recedes_unreached() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default());
        // Wavefront from main, currently at depth 0: main glows, handler (depth
        // 1, not yet reached) recedes hard.
        let depth = depth_field(4, &g.index.bfs(Direction::Outbound, &[0]));
        let s = node_state(&g, &c, None, AnimMode::Roots, &depth, 0);
        assert_eq!(&s[0..4], &[1.0, 0.9, 1.0, 0.0], "current depth glows");
        assert_eq!(s[4], ANIM_FADE_ALPHA, "ahead of the wavefront recedes");
        let s = node_state(&g, &c, None, AnimMode::Roots, &depth, 1);
        assert_eq!(&s[0..4], &[TRAIL_ALPHA, 0.0, 0.6, 0.0], "behind the wavefront trails");
    }

    #[test]
    fn edge_state_interleave_and_selection_accent() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default());
        let s = edge_state(&c, None, AnimMode::Off, &[], 0);
        assert_eq!(s.len(), c.edges.len() * 4, "4 f32 per uploaded edge");
        assert_eq!(&s[0..4], &[0.28, 0.0, 0.0, 0.0], "small-graph base alpha");
        let s = edge_state(&c, Some(1), AnimMode::Off, &[], 0);
        assert_eq!(&s[0..4], &[0.9, 1.0, 0.0, 0.0], "edge touching the selection accents");
    }

    #[test]
    fn static_interleaves_follow_the_upload_layout() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default());
        let positions = vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)];
        let (npr, ee) = static_interleaves(&g, &c, &positions);
        assert_eq!(npr.len(), c.nodes.len() * 3, "[x, y, radius] per node");
        assert_eq!(ee.len(), c.edges.len() * 4, "[fx, fy, tx, ty] per edge");
        assert_eq!(&npr[0..3], &[0.0, 0.0, g.radius[0]]);
        assert_eq!(&ee[0..4], &[0.0, 0.0, 10.0, 0.0], "main→handler endpoints");
    }

    #[test]
    fn fit_zoom_is_bounded_and_handles_empty_input() {
        let positions = vec![(0.0, 0.0), (100.0, 50.0), (-100.0, -50.0)];
        let z = fit_zoom(&positions, 1600.0, 1000.0);
        assert!((0.02..=3.0).contains(&z), "zoom clamped, got {z}");
        assert_eq!(fit_zoom(&[], 1600.0, 1000.0), 3.0, "empty layout clamps to max");
    }

    #[test]
    fn recompute_cull_clears_a_now_invisible_selection() {
        let g = build_graph(&fixture(), Tier::Functions);
        let sim = simulate(4, &g.layout_edges, 42, &ForceConfig { max_ticks: 5, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0);
        // Select the dead node while everything is shown, then cull it away.
        vs.filter.show_dead = true;
        vs.recompute_cull();
        vs.selected = Some(2);
        vs.filter = FilterState::default();
        vs.recompute_cull();
        assert_eq!(vs.selected, None, "a culled selection must not linger");
        assert_eq!(vs.cull.nodes, vec![0, 1]);
    }

    #[test]
    fn recompute_bfs_seeds_from_roots_or_selection() {
        let g = build_graph(&fixture(), Tier::Functions);
        let sim = simulate(4, &g.layout_edges, 42, &ForceConfig { max_ticks: 5, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0);

        vs.anim_mode = AnimMode::Roots;
        vs.recompute_bfs();
        assert_eq!(vs.anim_depth[0], 0);
        assert_eq!(vs.anim_depth[1], 1, "both roots seed depth 0; handler is 1 from main");
        assert_eq!(vs.anim_max_depth, 2, "main→handler→helper");

        vs.anim_mode = AnimMode::Inbound;
        vs.selected = Some(2);
        vs.recompute_bfs();
        assert_eq!(vs.anim_depth[2], 0, "inbound seeds at the selection");
        assert_eq!(vs.anim_depth[0], 2, "who reaches helper: handler, then main");
        assert_eq!(vs.anim_current_depth, 0);
    }
}
