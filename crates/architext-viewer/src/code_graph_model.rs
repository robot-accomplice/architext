//! Pure model + layout for the Code Graph mode (contract `magma-code-graph/1`).
//!
//! Mirrors the `blast_radius` / `diagram::sequence` split: no Leptos here — this
//! turns a fetched `CodeGraph` into already-positioned primitives that
//! `components::code_graph_svg` renders verbatim.
//!
//! Layout is a deterministic LAYERED assignment (BFS call-depth from roots),
//! not force-directed: a call graph is DIRECTED, and layering is what makes call
//! direction and reachability readable. There is deliberately no crossing
//! minimisation — legibility here comes from layering and from bounding the node
//! count per tier (modules first, one module's functions on drill-down).
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::data::models::{CodeGraph, CodeGraphFunction, CodeGraphSignature};

/// SVG node-card text metrics (mirrors `wrap_name` in `diagram/node.rs`): pure
/// string math against the card width, no DOM measurement. `LABEL_PAD_X`
/// matches the `x="10"` offset both `<text>` elements use in
/// `components::code_graph_svg`; the char-width constants are tuned to the
/// label's 13px and sublabel's 10px font sizes (`.cg-node__label` /
/// `.cg-node__sublabel` in styles.css) so a repo-relative Go `file:line`
/// path — the NORMAL case on the function tier — truncates instead of
/// spilling across the 190px card border.
const LABEL_PAD_X: f64 = 10.0;
const LABEL_CHAR_W: f64 = 13.0 * 0.55;
const SUBLABEL_CHAR_W: f64 = 10.0 * 0.5;

/// Max characters that fit `node_w` at the given per-character advance.
fn max_chars(node_w: f64, char_w: f64) -> usize {
    (((node_w - 2.0 * LABEL_PAD_X) / char_w).floor() as usize).max(1)
}

/// Ellipsis-truncate `s` to at most `cap` characters (char count, not bytes —
/// same convention as `wrap_name`). Strings within the cap pass through
/// untouched.
fn truncate_ellipsis(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let keep = cap.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Ids from `all_ids` that never appear as the `to` side of `pairs` — the
/// edge-derived root fallback shared by both tiers when no declared root
/// exists (module `fan_in == 0` / function `root`). A module or function can
/// carry `fan_in > 0` from calls elsewhere in the repo while still having no
/// caller WITHIN this subgraph; without this, `roots` comes up empty and
/// `assign_layers` collapses every id into one layer.
fn ids_not_called<'a>(
    all_ids: impl Iterator<Item = &'a str>,
    pairs: &[(String, String)],
) -> Vec<String> {
    let called: HashSet<&str> = pairs.iter().map(|(_, to)| to.as_str()).collect();
    all_ids.filter(|id| !called.contains(id)).map(|s| s.to_string()).collect()
}

/// Resolved layout dimensions. Mirrors `SequenceConfig`'s role.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphConfig {
    pub node_w: f64,
    pub node_h: f64,
    pub layer_gap: f64,
    pub node_gap: f64,
    pub margin: f64,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self { node_w: 190.0, node_h: 62.0, layer_gap: 90.0, node_gap: 22.0, margin: 40.0 }
    }
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub sublabel: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub badges: Vec<Reach>,
    pub fan_in: u32,
    pub fan_out: u32,
    /// True only on the coarse (module) tier.
    pub drillable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub id: String,
    /// SVG path data, consumed verbatim by the renderer (same contract as
    /// `diagram::edge`, which never builds paths client-side).
    pub d: String,
    /// Any underlying dispatch is dynamic (RTA over-approximation).
    pub dynamic: bool,
    /// Underlying fine call edges collapsed into this edge (1 at function tier).
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphLayout {
    pub content_width: f64,
    pub content_height: f64,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Assign each id a layer: BFS depth from the roots.
///
/// Cycle-safe (first-seen depth wins, so recursion terminates) and
/// deterministic (roots and `ids` are walked in document order). Ids never
/// reached from a root land in one trailing layer, so orphans and cycle-only
/// components stay visible instead of vanishing.
fn assign_layers(
    ids: &[String],
    roots: &[String],
    edges: &[(String, String)],
) -> HashMap<String, usize> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in edges {
        adj.entry(from.as_str()).or_default().push(to.as_str());
    }

    let mut layer: HashMap<String, usize> = HashMap::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    for r in roots {
        if !layer.contains_key(r) {
            layer.insert(r.clone(), 0);
            queue.push_back((r.clone(), 0));
        }
    }
    while let Some((id, depth)) = queue.pop_front() {
        let Some(next) = adj.get(id.as_str()) else { continue };
        for n in next {
            if !layer.contains_key(*n) {
                layer.insert((*n).to_string(), depth + 1);
                queue.push_back(((*n).to_string(), depth + 1));
            }
        }
    }

    let trailing = layer.values().copied().max().map(|m| m + 1).unwrap_or(0);
    for id in ids {
        layer.entry(id.clone()).or_insert(trailing);
    }
    layer
}

/// Place ids into columns by layer, rows by document order within the layer.
/// Returns positions plus the content bounds.
fn place(
    ids: &[String],
    layer: &HashMap<String, usize>,
    cfg: &GraphConfig,
) -> (HashMap<String, (f64, f64)>, f64, f64) {
    // BTreeMap: layer iteration order must be stable, or bounds could differ
    // between otherwise-identical runs.
    let mut by_layer: BTreeMap<usize, Vec<&String>> = BTreeMap::new();
    for id in ids {
        by_layer.entry(*layer.get(id).unwrap_or(&0)).or_default().push(id);
    }

    let mut pos = HashMap::new();
    let mut rows = 0usize;
    let mut layers = 0usize;
    for (l, members) in &by_layer {
        layers = layers.max(*l + 1);
        rows = rows.max(members.len());
        for (i, id) in members.iter().enumerate() {
            let x = cfg.margin + *l as f64 * (cfg.node_w + cfg.layer_gap);
            let y = cfg.margin + i as f64 * (cfg.node_h + cfg.node_gap);
            pos.insert((*id).clone(), (x, y));
        }
    }

    let width = if layers == 0 {
        0.0
    } else {
        cfg.margin * 2.0 + layers as f64 * cfg.node_w + (layers - 1) as f64 * cfg.layer_gap
    };
    let height = if rows == 0 {
        0.0
    } else {
        cfg.margin * 2.0 + rows as f64 * cfg.node_h + (rows - 1) as f64 * cfg.node_gap
    };
    (pos, width, height)
}

/// A left-to-right cubic bezier between two node edges. Call direction reads
/// L→R, so the control points are horizontal offsets. Back-edges (cycles) draw
/// as a returning curve, which is exactly the visual cue wanted.
fn bezier_d(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
    let dx = ((x2 - x1).abs() * 0.5).max(30.0);
    format!(
        "M {x1:.1} {y1:.1} C {:.1} {y1:.1}, {:.1} {y2:.1}, {x2:.1} {y2:.1}",
        x1 + dx,
        x2 - dx
    )
}

/// Build one edge between two placed nodes, anchored right-edge → left-edge.
fn edge_between(
    id: String,
    from: &str,
    to: &str,
    pos: &HashMap<String, (f64, f64)>,
    cfg: &GraphConfig,
    dynamic: bool,
    count: u32,
) -> Option<GraphEdge> {
    let (fx, fy) = *pos.get(from)?;
    let (tx, ty) = *pos.get(to)?;
    let d = bezier_d(
        fx + cfg.node_w,
        fy + cfg.node_h / 2.0,
        tx,
        ty + cfg.node_h / 2.0,
    );
    Some(GraphEdge { id, d, dynamic, count })
}

/// The COARSE tier: every module, edges from `module_calls`.
pub fn build_module_layout(cg: &CodeGraph, cfg: &GraphConfig) -> GraphLayout {
    let Some(modules) = cg.modules.as_ref() else { return GraphLayout::default() };
    let empty = Vec::new();
    let calls = cg.module_calls.as_ref().unwrap_or(&empty);

    let ids: Vec<String> = modules.iter().map(|m| m.id.clone()).collect();
    let pairs: Vec<(String, String)> =
        calls.iter().map(|c| (c.from.clone(), c.to.clone())).collect();
    // Roots: modules nothing calls into (fan_in is the module-graph degree).
    let mut roots: Vec<String> =
        modules.iter().filter(|m| m.fan_in == 0).map(|m| m.id.clone()).collect();
    if roots.is_empty() {
        // A pure inter-module cycle can leave fan_in > 0 for every module —
        // fall back to the edges actually seen here (mirrors the function
        // tier below) instead of silently collapsing every module into one
        // layer. If that ALSO comes up empty (a true full cycle), `roots`
        // stays empty and `assign_layers`'s trailing-layer catch-all still
        // places every node — just in one undifferentiated column.
        roots = ids_not_called(modules.iter().map(|m| m.id.as_str()), &pairs);
    }

    let layer = assign_layers(&ids, &roots, &pairs);
    let (pos, content_width, content_height) = place(&ids, &layer, cfg);

    let nodes = modules
        .iter()
        .filter_map(|m| {
            let (x, y) = *pos.get(&m.id)?;
            Some(GraphNode {
                id: m.id.clone(),
                label: truncate_ellipsis(&m.pkg, max_chars(cfg.node_w, LABEL_CHAR_W)),
                sublabel: truncate_ellipsis(
                    &format!(
                        "{} fn · {} dead · {} test-only",
                        m.counts.functions, m.counts.dead, m.counts.test_only
                    ),
                    max_chars(cfg.node_w, SUBLABEL_CHAR_W),
                ),
                x,
                y,
                w: cfg.node_w,
                h: cfg.node_h,
                badges: Vec::new(),
                fan_in: m.fan_in,
                fan_out: m.fan_out,
                drillable: true,
            })
        })
        .collect();

    let edges = calls
        .iter()
        .filter_map(|c| {
            edge_between(
                format!("{}->{}", c.from, c.to),
                &c.from,
                &c.to,
                &pos,
                cfg,
                c.has_dynamic,
                c.count,
            )
        })
        .collect();

    GraphLayout { content_width, content_height, nodes, edges }
}

/// The FINE tier: one module's functions and the calls between them.
/// Intra-module only — bounding node count is the point of the coarse tier.
pub fn build_function_layout(cg: &CodeGraph, module_id: &str, cfg: &GraphConfig) -> GraphLayout {
    let (Some(functions), Some(modules)) = (cg.functions.as_ref(), cg.modules.as_ref()) else {
        return GraphLayout::default();
    };
    let Some(module) = modules.iter().find(|m| m.id == module_id) else {
        return GraphLayout::default();
    };

    // Set lookups, not per-function/per-call linear scans: Magma targets Go
    // repos where 10k functions / 40k calls is ordinary, re-evaluated on every
    // `state.data` tick (SSE reload), not just on drill.
    let member_ids: HashSet<&str> = module.function_ids.iter().map(|id| id.as_str()).collect();
    let members: Vec<&CodeGraphFunction> =
        functions.iter().filter(|f| member_ids.contains(f.id.as_str())).collect();
    if members.is_empty() {
        return GraphLayout::default();
    }
    let ids: Vec<String> = members.iter().map(|f| f.id.clone()).collect();
    let ids_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    let empty = Vec::new();
    let inner: Vec<&crate::data::models::CodeGraphCall> = cg
        .calls
        .as_ref()
        .unwrap_or(&empty)
        .iter()
        .filter(|c| ids_set.contains(c.from.as_str()) && ids_set.contains(c.to.as_str()))
        .collect();
    let pairs: Vec<(String, String)> =
        inner.iter().map(|c| (c.from.clone(), c.to.clone())).collect();

    // Roots: declared entrypoints, else anything nothing in this module calls.
    let mut roots: Vec<String> =
        members.iter().filter(|f| f.root).map(|f| f.id.clone()).collect();
    if roots.is_empty() {
        roots = ids_not_called(members.iter().map(|f| f.id.as_str()), &pairs);
    }

    let layer = assign_layers(&ids, &roots, &pairs);
    let (pos, content_width, content_height) = place(&ids, &layer, cfg);

    let nodes = members
        .iter()
        .filter_map(|f| {
            let (x, y) = *pos.get(&f.id)?;
            Some(GraphNode {
                id: f.id.clone(),
                label: truncate_ellipsis(&f.symbol, max_chars(cfg.node_w, LABEL_CHAR_W)),
                sublabel: truncate_ellipsis(
                    &format!("{}:{}", f.file, f.line),
                    max_chars(cfg.node_w, SUBLABEL_CHAR_W),
                ),
                x,
                y,
                w: cfg.node_w,
                h: cfg.node_h,
                badges: reach_badges(f),
                fan_in: f.fan_in,
                fan_out: f.fan_out,
                drillable: false,
            })
        })
        .collect();

    let edges = inner
        .iter()
        .filter_map(|c| {
            edge_between(
                format!("{}->{}", c.from, c.to),
                &c.from,
                &c.to,
                &pos,
                cfg,
                c.kind == "dynamic",
                1,
            )
        })
        .collect();

    GraphLayout { content_width, content_height, nodes, edges }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::CodeGraph;

    /// Two modules, `a` calling `b`, with one function each.
    fn two_module_graph() -> CodeGraph {
        serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "magma/0.2.0", "language": "go", "module": "example.com/m",
            "sha": "abc", "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [
                {"id": "a-main", "symbol": "main", "pkg": "a", "file": "a.go", "line": 1,
                 "kind": "func", "exported": false, "test": false, "root": true,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 1},
                {"id": "a-helper", "symbol": "helper", "pkg": "a", "file": "a.go", "line": 9,
                 "kind": "func", "exported": false, "test": false, "root": false,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 1, "fan_out": 0},
                {"id": "b-run", "symbol": "Run", "pkg": "b", "file": "b.go", "line": 3,
                 "kind": "func", "exported": true, "test": false, "root": false,
                 "generated": false, "reachable": false, "prod_reachable": false,
                 "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 0}
            ],
            "calls": [
                {"from": "a-main", "to": "a-helper", "site_file": "a.go", "site_line": 4, "kind": "static"}
            ],
            "modules": [
                {"id": "a", "pkg": "a", "function_ids": ["a-main", "a-helper"],
                 "counts": {"functions": 2, "dead": 0, "test_only": 0}, "fan_in": 0, "fan_out": 1},
                {"id": "b", "pkg": "b", "function_ids": ["b-run"],
                 "counts": {"functions": 1, "dead": 1, "test_only": 0}, "fan_in": 1, "fan_out": 0}
            ],
            "module_calls": [
                {"from": "a", "to": "b", "count": 3, "has_dynamic": true}
            ]
        })).expect("fixture parses")
    }

    #[test]
    fn module_layout_layers_callers_left_of_callees() {
        // WHY: layering IS the legibility mechanism — if a callee shared a
        // column with its caller the call direction would be unreadable.
        let cg = two_module_graph();
        let cfg = GraphConfig::default();
        let layout = build_module_layout(&cg, &cfg);

        let a = layout.nodes.iter().find(|n| n.id == "a").expect("module a");
        let b = layout.nodes.iter().find(|n| n.id == "b").expect("module b");
        assert!(a.x < b.x, "caller module must sit left of its callee: {} !< {}", a.x, b.x);
        assert_eq!(layout.nodes.len(), 2);
        assert_eq!(layout.edges.len(), 1);
        assert!(layout.content_width > 0.0 && layout.content_height > 0.0);
    }

    #[test]
    fn module_edge_carries_count_and_dynamic_flag() {
        // WHY: `count` weights the edge and `has_dynamic` picks the marker —
        // dropping either silently flattens real information in the diagram.
        let layout = build_module_layout(&two_module_graph(), &GraphConfig::default());
        let e = &layout.edges[0];
        assert_eq!(e.count, 3);
        assert!(e.dynamic);
        assert!(e.d.starts_with('M'), "edge must carry a path `d`, got {:?}", e.d);
    }

    #[test]
    fn module_nodes_are_drillable_and_labelled_with_counts() {
        let layout = build_module_layout(&two_module_graph(), &GraphConfig::default());
        let b = layout.nodes.iter().find(|n| n.id == "b").unwrap();
        assert!(b.drillable, "module nodes drill into their functions");
        assert_eq!(b.label, "b");
        assert!(b.sublabel.contains('1'), "sublabel should carry counts, got {:?}", b.sublabel);
    }

    #[test]
    fn function_layout_is_scoped_to_one_module() {
        // WHY: the whole point of the coarse tier is bounding node count —
        // leaking other modules' functions in would defeat it.
        let layout = build_function_layout(&two_module_graph(), "a", &GraphConfig::default());
        let ids: Vec<&str> = layout.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), 2, "only module a's functions: {ids:?}");
        assert!(ids.contains(&"a-main") && ids.contains(&"a-helper"));
        assert!(!ids.contains(&"b-run"));
        // The one intra-module call is kept.
        assert_eq!(layout.edges.len(), 1);
        assert!(!layout.nodes.iter().any(|n| n.drillable), "function nodes do not drill further");
    }

    #[test]
    fn unknown_module_yields_an_empty_layout_not_a_panic() {
        let layout = build_function_layout(&two_module_graph(), "does-not-exist", &GraphConfig::default());
        assert!(layout.nodes.is_empty());
        assert!(layout.edges.is_empty());
    }

    #[test]
    fn layout_is_deterministic() {
        // WHY: HashMap iteration order is not stable; a layout that reshuffles
        // between renders would make the diagram flicker on every reactive tick.
        let cg = two_module_graph();
        let cfg = GraphConfig::default();
        let first = build_module_layout(&cg, &cfg);
        for _ in 0..25 {
            assert_eq!(build_module_layout(&cg, &cfg), first);
        }
    }

    #[test]
    fn cycles_do_not_hang_and_every_node_is_placed() {
        // WHY: call graphs contain recursion. A naive longest-path layering
        // would loop forever; every node must still land somewhere visible.
        let cg: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "g", "language": "go", "module": "m",
            "sha": "s", "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [], "calls": [],
            "modules": [
                {"id": "x", "pkg": "x", "function_ids": [],
                 "counts": {"functions": 0, "dead": 0, "test_only": 0}, "fan_in": 1, "fan_out": 1},
                {"id": "y", "pkg": "y", "function_ids": [],
                 "counts": {"functions": 0, "dead": 0, "test_only": 0}, "fan_in": 1, "fan_out": 1}
            ],
            "module_calls": [
                {"from": "x", "to": "y", "count": 1, "has_dynamic": false},
                {"from": "y", "to": "x", "count": 1, "has_dynamic": false}
            ]
        })).unwrap();
        let layout = build_module_layout(&cg, &GraphConfig::default());
        assert_eq!(layout.nodes.len(), 2, "both cycle members must be placed");
        assert_eq!(layout.edges.len(), 2);
        // WHY: the old assertions (len only) CANNOT fail when layering breaks
        // — a total collapse still has 2 nodes and 2 edges. A true 2-node
        // cycle has no callee-free module even under the edge-derived
        // fallback, so both correctly land in the same layer/column; assert
        // that explicitly, plus that they still get distinct rows rather
        // than literally overlapping.
        let x = layout.nodes.iter().find(|n| n.id == "x").expect("module x");
        let y = layout.nodes.iter().find(|n| n.id == "y").expect("module y");
        assert_eq!(x.x, y.x, "a true full cycle has no ordering — one column");
        assert_ne!(x.y, y.y, "same-layer members must still get distinct rows");
    }

    #[test]
    fn module_root_fallback_uses_edges_when_fan_in_is_never_zero() {
        // WHY: a real Magma graph can have fan_in > 0 for every module (an
        // inter-module cycle elsewhere in the repo) while THIS subgraph
        // (a -> b -> c) has a clear caller order. Before the fix, `roots`
        // (filtered on fan_in == 0) came up empty and EVERY module collapsed
        // into layer 0 — the silent layering collapse from the review. This
        // must go RED without the edge-derived fallback and GREEN with it.
        let cg: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "g", "language": "go", "module": "m",
            "sha": "s", "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [], "calls": [],
            "modules": [
                {"id": "a", "pkg": "a", "function_ids": [],
                 "counts": {"functions": 0, "dead": 0, "test_only": 0}, "fan_in": 1, "fan_out": 1},
                {"id": "b", "pkg": "b", "function_ids": [],
                 "counts": {"functions": 0, "dead": 0, "test_only": 0}, "fan_in": 1, "fan_out": 1},
                {"id": "c", "pkg": "c", "function_ids": [],
                 "counts": {"functions": 0, "dead": 0, "test_only": 0}, "fan_in": 1, "fan_out": 0}
            ],
            "module_calls": [
                {"from": "a", "to": "b", "count": 1, "has_dynamic": false},
                {"from": "b", "to": "c", "count": 1, "has_dynamic": false}
            ]
        })).expect("fixture parses");

        let layout = build_module_layout(&cg, &GraphConfig::default());
        let a = layout.nodes.iter().find(|n| n.id == "a").expect("module a");
        let b = layout.nodes.iter().find(|n| n.id == "b").expect("module b");
        let c = layout.nodes.iter().find(|n| n.id == "c").expect("module c");
        assert!(
            a.x < b.x && b.x < c.x,
            "edge-derived root fallback must still layer callers left of callees: {} {} {}",
            a.x,
            b.x,
            c.x
        );
    }

    #[test]
    fn function_tier_pure_cycle_places_every_node() {
        // WHY: the module-tier cycle test above does not exercise the FUNCTION
        // tier's root-selection fallback. Here every function calls another
        // (a pure cycle), so no function has `root: true` AND every function
        // has a caller — both root sources come up empty. `assign_layers`
        // must still place every function instead of hanging or dropping any.
        let cg: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "g", "language": "go", "module": "m",
            "sha": "s", "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [
                {"id": "p-one", "symbol": "One", "pkg": "p", "file": "p.go", "line": 1,
                 "kind": "func", "exported": true, "test": false, "root": false,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 1, "fan_out": 1},
                {"id": "p-two", "symbol": "Two", "pkg": "p", "file": "p.go", "line": 9,
                 "kind": "func", "exported": true, "test": false, "root": false,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 1, "fan_out": 1},
                {"id": "p-three", "symbol": "Three", "pkg": "p", "file": "p.go", "line": 17,
                 "kind": "func", "exported": true, "test": false, "root": false,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 1, "fan_out": 1}
            ],
            "calls": [
                {"from": "p-one", "to": "p-two", "site_file": "p.go", "site_line": 2, "kind": "static"},
                {"from": "p-two", "to": "p-three", "site_file": "p.go", "site_line": 10, "kind": "static"},
                {"from": "p-three", "to": "p-one", "site_file": "p.go", "site_line": 18, "kind": "static"}
            ],
            "modules": [
                {"id": "p", "pkg": "p", "function_ids": ["p-one", "p-two", "p-three"],
                 "counts": {"functions": 3, "dead": 0, "test_only": 0}, "fan_in": 0, "fan_out": 0}
            ],
            "module_calls": []
        })).expect("fixture parses");

        let layout = build_function_layout(&cg, "p", &GraphConfig::default());

        let ids: Vec<&str> = layout.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "every function in the pure cycle must still be placed: {ids:?}");
        assert!(ids.contains(&"p-one") && ids.contains(&"p-two") && ids.contains(&"p-three"));
        assert_eq!(layout.edges.len(), 3, "all three intra-module calls must be kept");
    }

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
            let f: crate::data::models::CodeGraphFunction = serde_json::from_value(v).unwrap();
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
        use crate::data::models::CodeGraphSignature;
        let sig: CodeGraphSignature = serde_json::from_value(serde_json::json!({
            "params": [{"name": "a", "type": "int"}, {"type": "string"}],
            "results": [{"type": "int"}, {"type": "error"}]
        })).unwrap();
        assert_eq!(format_signature(&sig), "(a int, string) (int, error)");

        let empty: CodeGraphSignature =
            serde_json::from_value(serde_json::json!({"params": [], "results": []})).unwrap();
        assert_eq!(format_signature(&empty), "()");

        let one: CodeGraphSignature = serde_json::from_value(serde_json::json!({
            "params": [], "results": [{"type": "error"}]
        })).unwrap();
        assert_eq!(format_signature(&one), "() error");
    }

    #[test]
    fn long_function_sublabel_is_ellipsis_truncated() {
        // WHY: repo-relative Go paths (`file:line`) are the NORMAL case on
        // the function tier and routinely overflow the 190px node card at
        // 10px font — this is the review's IMPORTANT #1 (label spill across
        // the card border), fixed in the model so it stays unit-testable.
        let cg: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1",
            "generator": "g", "language": "go", "module": "m",
            "sha": "s", "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": [
                {"id": "p-long", "symbol": "Handle", "pkg": "p",
                 "file": "internal/service/reconciliation/orchestrator/handler.go", "line": 482,
                 "kind": "func", "exported": true, "test": false, "root": true,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 0},
                {"id": "p-short", "symbol": "Run", "pkg": "p", "file": "p.go", "line": 3,
                 "kind": "func", "exported": true, "test": false, "root": true,
                 "generated": false, "reachable": true, "prod_reachable": true,
                 "signature": {"params": [], "results": []}, "fan_in": 0, "fan_out": 0}
            ],
            "calls": [],
            "modules": [
                {"id": "p", "pkg": "p", "function_ids": ["p-long", "p-short"],
                 "counts": {"functions": 2, "dead": 0, "test_only": 0}, "fan_in": 0, "fan_out": 0}
            ],
            "module_calls": []
        })).expect("fixture parses");

        let layout = build_function_layout(&cg, "p", &GraphConfig::default());
        let long = layout.nodes.iter().find(|n| n.id == "p-long").expect("p-long");
        let short = layout.nodes.iter().find(|n| n.id == "p-short").expect("p-short");

        let raw_long = "internal/service/reconciliation/orchestrator/handler.go:482";
        assert!(
            long.sublabel.ends_with('…'),
            "long path must be ellipsis-truncated, got {:?}",
            long.sublabel
        );
        assert!(
            long.sublabel.chars().count() < raw_long.chars().count(),
            "truncated sublabel must actually be shorter: {:?}",
            long.sublabel
        );
        assert_eq!(short.sublabel, "p.go:3", "short path must pass through untouched");
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
