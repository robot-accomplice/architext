//! Pure code-graph model: adjacency, BFS wavefront, and culling filters.
//!
//! Zero Leptos, zero web-sys — this is the graph-reasoning core the WebGL
//! renderer (Plan C) builds on, split out so it is fully unit-testable
//! without a browser or WASM target. Lifted from the proven spike at
//! `code_graph_gl.rs` (`bfs`, `AnimMode` source selection, `ViewState`
//! filtering) and restructured into a clean, reusable shape; the algorithms
//! are not reinvented here, only the layering.
//!
//! `Reach`, `reach_badges`, and `format_signature` also live here — moved
//! (not duplicated) from `code_graph_model.rs`, which the inspector still
//! depends on for layout but which is scheduled for deletion once the WebGL
//! renderer replaces the SVG/CSS one.
use crate::data::models::{CodeGraphFunction, CodeGraphSignature};

// ─── adjacency + BFS wavefront ─────────────────────────────────────────────

/// Traversal direction for [`GraphIndex::bfs`].
///
/// `Outbound` follows `from -> to` (what this node calls); `Inbound` follows
/// edges BACKWARDS (who reaches this node). Getting this reversed is silent
/// and wrong — see `bfs_inbound_traverses_edges_backwards` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outbound,
    Inbound,
}

/// Forward + reverse adjacency over a directed graph on `0..node_count`,
/// built once from an edge list keyed by index (never string ids — the
/// caller maps ids to indices once so this stays cheap at 17,561 nodes /
/// 49,368 edges).
pub struct GraphIndex {
    node_count: usize,
    forward_adj: Vec<Vec<usize>>,
    reverse_adj: Vec<Vec<usize>>,
    out_degree: Vec<u32>,
    in_degree: Vec<u32>,
}

impl GraphIndex {
    /// Build the index from a directed edge list `(from, to)` over
    /// `0..node_count`. Out-of-range endpoints are dropped rather than
    /// panicking — callers that filter/remap ids can otherwise produce a
    /// transient out-of-range pair without crashing the viewer.
    pub fn from_edges(node_count: usize, edges: &[(usize, usize)]) -> Self {
        let mut forward_adj = vec![Vec::new(); node_count];
        let mut reverse_adj = vec![Vec::new(); node_count];
        let mut out_degree = vec![0u32; node_count];
        let mut in_degree = vec![0u32; node_count];
        for &(from, to) in edges {
            if from >= node_count || to >= node_count {
                continue;
            }
            forward_adj[from].push(to);
            reverse_adj[to].push(from);
            out_degree[from] += 1;
            in_degree[to] += 1;
        }
        Self { node_count, forward_adj, reverse_adj, out_degree, in_degree }
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn out_degree(&self, node: usize) -> u32 {
        self.out_degree[node]
    }

    pub fn in_degree(&self, node: usize) -> u32 {
        self.in_degree[node]
    }

    /// Total degree (fan-in + fan-out) — the sizing signal node radii key off.
    pub fn degree(&self, node: usize) -> u32 {
        self.out_degree[node] + self.in_degree[node]
    }

    /// Multi-source BFS, returning depth-ordered layers: `layers[0]` is the
    /// seed set, `layers[d]` is every node first reached at depth `d`.
    /// Cycle-safe — each node is visited exactly once, first-seen depth wins.
    pub fn bfs(&self, dir: Direction, seeds: &[usize]) -> Vec<Vec<usize>> {
        let adj = match dir {
            Direction::Outbound => &self.forward_adj,
            Direction::Inbound => &self.reverse_adj,
        };
        let mut visited = vec![false; self.node_count];
        let mut frontier: Vec<usize> = Vec::new();
        for &s in seeds {
            if s < self.node_count && !visited[s] {
                visited[s] = true;
                frontier.push(s);
            }
        }

        let mut layers = Vec::new();
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for &u in &frontier {
                for &v in &adj[u] {
                    if !visited[v] {
                        visited[v] = true;
                        next.push(v);
                    }
                }
            }
            layers.push(frontier);
            frontier = next;
        }
        layers
    }
}

// ─── culling filters ────────────────────────────────────────────────────────

/// Which reachability classes and edge kinds are currently shown.
///
/// `default()` culls to production-reachable only, per maintainer decision:
/// filtering is ON by default so the view never opens as a hairball.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterState {
    pub show_prod_reachable: bool,
    pub show_dead: bool,
    pub show_test_only: bool,
    pub show_generated: bool,
    pub show_static: bool,
    pub show_dynamic: bool,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            show_prod_reachable: true,
            show_dead: false,
            show_test_only: false,
            show_generated: false,
            show_static: true,
            show_dynamic: true,
        }
    }
}

impl FilterState {
    /// Per-node visibility: a node is visible if it matches ANY class this
    /// state currently shows. The four slices are parallel, index-aligned
    /// with the node list (same shape as the spike's `GraphData` flags).
    pub fn visible_nodes(
        &self,
        prod_reachable: &[bool],
        dead: &[bool],
        test_only: &[bool],
        generated: &[bool],
    ) -> Vec<bool> {
        let n = prod_reachable.len();
        (0..n)
            .map(|i| {
                (self.show_prod_reachable && prod_reachable[i])
                    || (self.show_dead && dead[i])
                    || (self.show_test_only && test_only[i])
                    || (self.show_generated && generated[i])
            })
            .collect()
    }

    /// Edge-kind visibility: static vs. dynamic dispatch.
    pub fn edge_visible(&self, dynamic: bool) -> bool {
        if dynamic {
            self.show_dynamic
        } else {
            self.show_static
        }
    }
}

// ─── reachability badges (moved from `code_graph_model.rs`) ────────────────

/// A CANDIDATE reachability classification.
///
/// Candidates, never verdicts: reflection, `encoding/json` interface
/// dispatch, cgo, and externally-invoked entrypoints all hide callers from
/// static analysis, so an unreached function is not proof of dead code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Root,
    Generated,
    TestOnly,
    Dead,
}

impl Reach {
    pub fn label(self) -> &'static str {
        match self {
            Reach::Root => "root",
            Reach::Generated => "generated",
            Reach::TestOnly => "test-only",
            Reach::Dead => "dead",
        }
    }

    /// The CSS custom property carrying this badge's tint.
    ///
    /// A dedicated `--reach-*` scale: these are neither a role identity hue
    /// (`--c4-*`) nor a state signal (`--accent`), so per DESIGN.md rule 1 they
    /// get their own channel — exactly as `--sev-*` / `--sens-*` were split out.
    pub fn color_var(self) -> &'static str {
        match self {
            Reach::Root => "var(--reach-root)",
            Reach::Generated => "var(--reach-generated)",
            Reach::TestOnly => "var(--reach-test-only)",
            Reach::Dead => "var(--reach-dead)",
        }
    }

    /// Hover text. For the two INFERRED classifications this must name the
    /// blind spots, so a badge is never read as a verdict.
    pub fn tooltip(self) -> &'static str {
        match self {
            Reach::Root => "Entrypoint: invoked from outside the analysed module.",
            Reach::Generated => "Generated code — excluded from dead-code candidates.",
            Reach::TestOnly => "CANDIDATE: reached only from tests. Reflection, encoding/json \
                interfaces, cgo and external entrypoints can hide production callers.",
            Reach::Dead => "CANDIDATE: no static caller found. Reflection, encoding/json \
                interfaces, cgo and external entrypoints can hide callers — verify before deleting.",
        }
    }
}

/// The candidate classifications for one function, per the contract's own
/// predicates (`dead` and `test_only` are Magma's definitions — do not drift).
pub fn reach_badges(f: &CodeGraphFunction) -> Vec<Reach> {
    let mut out = Vec::new();
    if f.root {
        out.push(Reach::Root);
    }
    if f.generated {
        out.push(Reach::Generated);
    }
    if !f.reachable && !f.generated && !f.root {
        out.push(Reach::Dead);
    }
    if f.reachable && !f.prod_reachable && !f.test && !f.root && !f.generated {
        out.push(Reach::TestOnly);
    }
    out
}

/// Render a Go-style signature: `(a int, string) (int, error)`.
/// A single unparenthesised result matches how Go itself prints one.
pub fn format_signature(sig: &CodeGraphSignature) -> String {
    let params = sig
        .params
        .iter()
        .map(|p| match &p.name {
            Some(n) => format!("{n} {}", p.param_type),
            None => p.param_type.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let results: Vec<&str> = sig.results.iter().map(|r| r.result_type.as_str()).collect();
    match results.len() {
        0 => format!("({params})"),
        1 => format!("({params}) {}", results[0]),
        _ => format!("({params}) ({})", results.join(", ")),
    }
}

// ─── test-only fixtures shared across this module's tests ─────────────────

/// Deterministic generators reused by tests that need real scale (≥1000
/// nodes with hubs, not a uniform mesh) to catch defects invisible below
/// ~50 nodes. Ported from `force_layout.rs`'s test module (Task 1) rather
/// than imported — that generator is private to force_layout's own tests
/// and this module must stay independently unit-testable.
#[cfg(test)]
pub(crate) mod tests_support {
    /// Build a connected scale-free-ish graph: `n` nodes, each new node
    /// attaching to `m` earlier ones (preferential attachment), so the result
    /// has hubs and a realistic degree distribution rather than a uniform mesh.
    pub fn interconnected(n: usize, m: usize) -> (usize, Vec<(usize, usize)>) {
        let mut edges = Vec::new();
        let mut targets: Vec<usize> = vec![0];
        for v in 1..n {
            for k in 0..m.min(targets.len()) {
                // deterministic pick — no RNG, so the fixture is reproducible
                let t = targets[(v * 7 + k * 13) % targets.len()];
                if t != v {
                    edges.push((t, v));
                    targets.push(t);
                }
            }
            targets.push(v);
        }
        (n, edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bfs_from_roots_orders_by_call_depth() {
        // root -> a -> b ; root -> c
        let g = GraphIndex::from_edges(4, &[(0, 1), (1, 2), (0, 3)]);
        let layers = g.bfs(Direction::Outbound, &[0]);
        assert_eq!(layers[0], vec![0]);
        assert_eq!(layers[1], vec![1, 3]);
        assert_eq!(layers[2], vec![2]);
    }

    #[test]
    fn bfs_inbound_traverses_edges_backwards() {
        // WHY: "who reaches this?" is the debugging question and it is the
        // reverse of the call direction — getting it backwards is silent.
        let g = GraphIndex::from_edges(4, &[(0, 1), (1, 2), (0, 3)]);
        let layers = g.bfs(Direction::Inbound, &[2]);
        assert_eq!(layers[0], vec![2]);
        assert_eq!(layers[1], vec![1]);
        assert_eq!(layers[2], vec![0]);
    }

    #[test]
    fn bfs_terminates_on_cycles() {
        let g = GraphIndex::from_edges(3, &[(0, 1), (1, 2), (2, 0)]);
        let layers = g.bfs(Direction::Outbound, &[0]);
        let total: usize = layers.iter().map(|l| l.len()).sum();
        assert_eq!(total, 3, "each node visited exactly once");
    }

    #[test]
    fn bfs_handles_1000_interconnected_nodes() {
        let (n, edges) = tests_support::interconnected(1000, 3);
        let g = GraphIndex::from_edges(n, &edges);
        let layers = g.bfs(Direction::Outbound, &[0]);
        let total: usize = layers.iter().map(|l| l.len()).sum();
        assert!(total > 900, "expected most of the graph reachable, got {total}");
        assert!(layers.len() > 2, "expected real depth, got {}", layers.len());
    }

    #[test]
    fn default_filter_culls_non_prod_reachable() {
        // The default view is what actually runs in production.
        let f = FilterState::default();
        assert!(f.show_prod_reachable);
        assert!(!f.show_dead, "dead-code candidates are hidden by default");
        assert!(!f.show_test_only);
        assert!(!f.show_generated);
    }

    #[test]
    fn visible_nodes_matches_any_shown_class() {
        // node 0: prod-reachable only; node 1: dead only; node 2: test-only;
        // node 3: generated; node 4: none of the four (e.g. root-only).
        let prod = vec![true, false, false, false, false];
        let dead = vec![false, true, false, false, false];
        let test_only = vec![false, false, true, false, false];
        let generated = vec![false, false, false, true, false];

        let default_visible = FilterState::default().visible_nodes(&prod, &dead, &test_only, &generated);
        assert_eq!(default_visible, vec![true, false, false, false, false]);

        let show_all = FilterState {
            show_prod_reachable: true,
            show_dead: true,
            show_test_only: true,
            show_generated: true,
            show_static: true,
            show_dynamic: true,
        };
        let all_visible = show_all.visible_nodes(&prod, &dead, &test_only, &generated);
        assert_eq!(all_visible, vec![true, true, true, true, false]);
    }

    #[test]
    fn edge_visible_respects_static_and_dynamic_flags() {
        let mut f = FilterState::default();
        assert!(f.edge_visible(false) && f.edge_visible(true), "both kinds shown by default");
        f.show_dynamic = false;
        assert!(f.edge_visible(false) && !f.edge_visible(true));
    }

    // --- Reach / reach_badges / format_signature (moved from code_graph_model.rs) ---

    #[test]
    fn reach_badges_follow_the_contract_predicates() {
        // WHY: these predicates are the contract's, not ours — dead and
        // test_only are defined by Magma and must not drift.
        let f = |reachable, prod, test, root, generated| {
            let v = serde_json::json!({
                "id": "f", "symbol": "F", "pkg": "p", "file": "f.go", "line": 1,
                "kind": "func", "exported": true, "test": test, "root": root,
                "generated": generated, "reachable": reachable, "prod_reachable": prod,
                "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 0
            });
            let f: CodeGraphFunction = serde_json::from_value(v).unwrap();
            reach_badges(&f)
        };

        // dead = !reachable && !generated && !root
        assert!(f(false, false, false, false, false).contains(&Reach::Dead));
        assert!(!f(false, false, false, true, false).contains(&Reach::Dead), "a root is never dead");
        assert!(!f(false, false, false, false, true).contains(&Reach::Dead), "generated is never dead");
        // test_only = reachable && !prod_reachable && !test && !root && !generated
        assert!(f(true, false, false, false, false).contains(&Reach::TestOnly));
        assert!(!f(true, false, true, false, false).contains(&Reach::TestOnly), "a test is not test-only");
        assert!(!f(true, true, false, false, false).contains(&Reach::TestOnly), "prod-reachable is not test-only");
        // markers
        assert!(f(true, true, false, true, false).contains(&Reach::Root));
        assert!(f(true, true, false, false, true).contains(&Reach::Generated));
    }

    #[test]
    fn signature_renders_named_and_unnamed_params() {
        let sig: CodeGraphSignature = serde_json::from_value(serde_json::json!({
            "params": [{"name": "a", "type": "int"}, {"type": "string"}],
            "results": [{"type": "int"}, {"type": "error"}]
        }))
        .unwrap();
        assert_eq!(format_signature(&sig), "(a int, string) (int, error)");

        let empty: CodeGraphSignature =
            serde_json::from_value(serde_json::json!({"params": [], "results": []})).unwrap();
        assert_eq!(format_signature(&empty), "()");

        let one: CodeGraphSignature = serde_json::from_value(serde_json::json!({
            "params": [], "results": [{"type": "error"}]
        }))
        .unwrap();
        assert_eq!(format_signature(&one), "() error");
    }

    #[test]
    fn every_badge_tooltip_says_it_is_a_candidate_for_the_inferred_ones() {
        // WHY: dead/test-only are STATIC-ANALYSIS CANDIDATES. A tooltip that
        // reads as a verdict invites someone to delete live code.
        for r in [Reach::Dead, Reach::TestOnly] {
            let t = r.tooltip().to_lowercase();
            assert!(t.contains("candidate"), "{:?} tooltip must say candidate: {t}", r);
            assert!(t.contains("reflection"), "{:?} tooltip must name a blind spot: {t}", r);
        }
        for r in [Reach::Root, Reach::Generated, Reach::TestOnly, Reach::Dead] {
            assert!(!r.label().is_empty());
            assert!(r.color_var().starts_with("var(--reach-"), "{:?} needs its own scale", r);
        }
    }
}
