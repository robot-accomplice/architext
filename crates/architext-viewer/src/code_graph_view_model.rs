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
use crate::force_layout::ForceConfig;
use crate::code_graph_graph::{Direction, FilterState, GraphIndex};
use crate::data::models::CodeGraph;
use crate::force_layout::QuadTree;

// `pub` (not just crate-private): after the decay rework below, this value's
// only remaining Rust-side reference is as the node decay floor argument
// passed to `decay_brightness` in tests (the render path itself only
// uploads raw age — see `hop_age`'s doc — the floor lives as a GLSL literal
// in `gl/shaders.rs`'s `NODE_VS`), so it needs to stay visible outside this
// module's non-test code to avoid a dead-code false positive.
pub const TRAIL_ALPHA: f32 = 0.35;
const SELECT_FADE_ALPHA: f32 = 0.05;

/// How many hop-durations a revealed node's brightness takes to decay from
/// peak down to its resting floor (`TRAIL_ALPHA`/`NODE_MIX_FLOOR`) — the
/// maintainer's ask, verbatim: "keep the new elements bright and gradually
/// fade out the older elements so that users can better see what's
/// happening" (the wavefront was previously an ever-accumulating STATIC
/// mass once a node passed its one hop of "just arrived" brightness — see
/// `node_state`'s pre-decay two-bucket alpha). 3 hops reads as "a few hops"
/// against `HOP_DURATION_MS` (1.5s/hop, so a 4.5s trail): long enough that a
/// reader mid-animation can still tell "revealed a hop or two ago" from
/// "long settled", short enough that most of the accumulated graph reads as
/// settled background rather than perpetually "recent". `hop_age` derives
/// the per-element age this decays against; `decay_brightness` is the curve
/// itself (the Rust-side, unit-tested twin of `gl/shaders.rs`'s `NODE_VS`,
/// which does the actual per-frame evaluation on the GPU — see its doc for
/// why the real work lives there and not here). Edges use a shorter span —
/// see `EDGE_DECAY_HOPS`.
pub const NODE_DECAY_HOPS: f32 = 3.0;

/// Edge counterpart of `NODE_DECAY_HOPS`, deliberately shorter (1.5 hops,
/// half a node's span). An edge's `progress` (see `edge_state`) already
/// encodes travel direction by growing from the reached endpoint toward the
/// next hop; keeping only the most recent hop or two lit is what keeps THAT
/// leading edge visually distinct from edges revealed three, four, five
/// hops back — a slower decay would smear many hops of direction cues into
/// one undifferentiated tangle. Nodes have no direction to blur, so their
/// afterglow is free to linger longer (`NODE_DECAY_HOPS`).
pub const EDGE_DECAY_HOPS: f32 = 1.5;

/// Resting colorMix a fully-decayed node settles to — exactly the flat
/// value `node_state` used to assign outright before decay existed, now
/// named because `gl/shaders.rs`'s `NODE_VS` also needs it (as a GLSL
/// literal — a shader source string cannot import a Rust `const`, so keep
/// the two in sync by hand if this ever changes).
pub const NODE_MIX_FLOOR: f32 = 0.6;

/// Resting alpha/colorMix a fully-decayed edge settles to — exactly the
/// flat values `edge_state` used to assign outright before decay existed.
/// See `NODE_MIX_FLOOR`'s doc: `gl/shaders.rs`'s `EDGE_VS` mirrors these by
/// hand.
pub const EDGE_ALPHA_FLOOR: f32 = 0.22;
pub const EDGE_MIX_FLOOR: f32 = 0.5;

/// Continuous hop-position of the wavefront's leading edge (`current_depth`
/// plus the fractional progress into the NEXT hop — the same quantity
/// `advance_hop` already derives every frame) minus an element's own BFS
/// depth (nodes), or its higher-depth endpoint (edges — see `edge_state`):
/// "how many hop-durations ago did the wavefront pass this element",
/// entirely reconstructed from state `ViewState` already tracks
/// (`anim_current_depth`/`anim_hop_progress`/`anim_depth`) rather than a
/// second, independently-ticking per-element timestamp — one animation
/// clock, not two. Negative (not yet fully reached, or — edges only — still
/// mid-growth) is deliberately left negative rather than clamped here:
/// `decay_brightness` treats any `age <= 0` as "freshly activated, full
/// brightness", so the in-flight growth band gets the same brightest
/// treatment as the instant-of-arrival without a separate branch.
fn hop_age(element_depth: i32, current_depth: i32, hop_progress: f32) -> f32 {
    (current_depth as f32 + hop_progress) - element_depth as f32
}

/// Linear peak-to-floor brightness fraction for an element `age` hop-
/// durations past activation (see `hop_age`). `age <= 0` is the freshest
/// state (full brightness — never a premature dim before an element has
/// actually arrived, and the same treatment an in-flight edge's negative
/// age gets); the fraction then ramps linearly down to `floor` over
/// `decay_span_hops` and holds flat. Linear, not eased/exponential: this is
/// a "wavefront moving through an accumulating mass" legibility cue, not a
/// decorative glow, and a linear ramp keeps the trail's visible LENGTH
/// constant in hop-space regardless of playback speed — an eased curve's
/// slow tail would make "how many hops back is still visible" depend on the
/// eye's contrast sensitivity rather than a fixed, predictable count.
///
/// This function is the CANONICAL reference for the identical decay math
/// `gl/shaders.rs`'s `NODE_VS`/`EDGE_VS` implement in GLSL (`mix(peak,
/// floor, t)` where `t = clamp(age / decay_span_hops, 0, 1)`). The real
/// per-frame evaluation runs there, on the GPU, once per vertex, every
/// frame — see `gl/renderer.rs`'s doc on why: recomputing a brightness
/// curve on the CPU across 17,814 nodes / 50,215 edges every animation
/// frame is exactly the cost this renderer's instanced-draw design exists
/// to avoid. This Rust twin exists purely so the curve's PROPERTIES
/// (monotonic, floor-respecting, brightest-first) are unit-testable without
/// a browser, since a GLSL source string cannot be. Keep the two in sync by
/// eye if `decay_span_hops`/`floor` ever change on either side.
pub fn decay_brightness(age_hops: f32, decay_span_hops: f32, floor: f32) -> f32 {
    if age_hops <= 0.0 {
        return 1.0;
    }
    let t = (age_hops / decay_span_hops).min(1.0);
    1.0 - t * (1.0 - floor)
}

/// Readable-trace pacing (maintainer directive, verbatim: "let's start with
/// readable trace... 'draw' them at that pace from one node to the next").
/// Roughly 1-2s per hop — slow enough to follow one call into the next,
/// short enough to watch a full run (BFS depth ~5 on the real corpus) to the
/// end. The single central constant `advance_hop` paces against; there is no
/// per-mode override because all three animation modes (roots/outbound/
/// inbound) are the same kind of BFS wavefront.
pub const HOP_DURATION_MS: f64 = 1500.0;

/// `prefers-reduced-motion: reduce` fallback cadence: matches the pre-rework
/// behaviour exactly — a whole depth layer reveals at once, no per-edge
/// growth. Kept as its own constant (not reused from `HOP_DURATION_MS`) since
/// the two paths are independently tunable and the reduced-motion contract is
/// "the CURRENT instant reveal", not "the new pace without growth".
pub const REDUCED_MOTION_HOP_DURATION_MS: f64 = 400.0;

/// The two granularities the view renders. Functions is the default tier
/// (the one that auto-plays the roots animation on open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Modules,
    Functions,
}

/// Fixed layout seed — reproducible layout across runs, tiers, and (Plan D
/// Task 3) main-thread vs. worker settles. Centralised here (rather than a
/// literal repeated at each settle call site) because the worker-warm path
/// (`layout_worker_client.rs`) now needs the SAME value the view's own
/// settle uses: a divergent seed would settle to a different (still valid,
/// but not cache-shareable) layout.
pub const LAYOUT_SEED: u64 = 1_469_598_103_934_665_603;

/// Whether entering `tier` should auto-play the roots animation once its
/// layout settles. Maintainer spec: the function tier only — modules stay
/// static (small and legible without a wavefront). Pulled out as its own
/// pure predicate so the trigger condition is unit-testable without a
/// browser: the view (`code_graph_view.rs`) calls this instead of inlining
/// the tier comparison at the RAF-loop settle site.
pub fn should_autoplay(tier: Tier) -> bool {
    tier == Tier::Functions
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
    /// Which cluster each node belongs to, and how many clusters there are.
    ///
    /// Carried ON the model because the layout is settled in two separate
    /// places -- the worker warm and the view's local settle -- and both need
    /// it. Deriving it at each call site meant wiring one and forgetting the
    /// other, which left the view rendering an unclustered sphere while the
    /// code read as though clustering had shipped.
    pub clusters: Vec<usize>,
    pub cluster_count: usize,
}

impl GraphModel {
    pub fn node_count(&self) -> usize {
        self.labels.len()
    }
}

/// Smallest rendered node radius, in model-space px — a degree-0 node.
const RADIUS_MIN: f32 = 3.0;
/// Largest rendered node radius. Above this, hubs start swallowing the canvas.
const RADIUS_MAX: f32 = 22.0;
/// Model-space px added per doubling of degree. Chosen so the real magma
/// corpus's busiest node (degree 888) lands just at `RADIUS_MAX` rather than
/// far past it, which is what keeps the top of the range readable instead of
/// saturated — see the measurements in `radius_separates_the_common_range`.
const RADIUS_PX_PER_DOUBLING: f32 = 2.0;

/// Node radius keys off total degree (fan-in + fan-out), clamped so hubs
/// dominate without swallowing the canvas.
///
/// LOG, not sqrt. The original `3.0 + sqrt(degree) * 1.7` is the textbook
/// "area proportional to value" mapping, and it is the wrong tool for THIS
/// distribution: real call graphs are heavy-tailed, and measuring the magma
/// corpus (17,814 Go functions) showed median degree 3 against a max of 888.
/// Under sqrt that put 77% of nodes inside a 3-6px band — three quarters of
/// the graph rendered at visually indistinguishable sizes — while clamping
/// flattened the 46 busiest nodes to an identical 22px, so the hubs a reader
/// most wants to pick out were exactly the ones the formula could not
/// separate. Saturating at both ends is the worst of both.
///
/// A log map spends the visual range where the nodes actually are: it widens
/// the crowded low-degree band and stops the top saturating (46 nodes at the
/// ceiling drops to 2). Degree 3 -> 7.0px, 10 -> 9.9, 38 -> 13.6, 100 -> 16.3,
/// 888 -> 22.0.
///
/// Deliberately NOT rank/percentile-based, which would use ~95% of the span:
/// that makes radius mean "busier than N% of THIS document", so the same
/// function changes size between repositories and a median node reads as a
/// hub. Absolute, comparable meaning is worth the narrower spread.
fn radius_for(degree: u32) -> f32 {
    let r = RADIUS_MIN + (1.0 + degree as f32).log2() * RADIUS_PX_PER_DOUBLING;
    r.clamp(RADIUS_MIN, RADIUS_MAX)
}

/// Build one tier's [`GraphModel`] from the Magma document.
/// Radius a lobe is allotted per sqrt(member).
///
/// NOT `k`. `k` is the ideal EDGE length (60), and using it here reserved
/// about two and a half times the room a lobe actually settles into: a
/// 634-member cluster claimed a 1,511-unit radius to hold members that occupy
/// roughly 600. Multiplied across 86 lobes the field inflated until the graph
/// read as a star chart -- correctly separated, and far too sparse to use.
///
/// Calibrated against the radius members really occupy under the anchor
/// spring, so a territory is the size of its contents rather than a multiple
/// of an unrelated constant.
const LOBE_RADIUS_K: f64 = 24.0;

/// Compaction rounds, each followed by re-separation. More rounds squeeze
/// harder; the packing stops shrinking once separation blocks it.
const CLUSTER_COMPACTION_PASSES: usize = 24;
/// How much of the distance to the centroid a round removes.
const COMPACTION_STEP: f64 = 0.88;

/// Passes that push cluster centres apart until each has room for its members.
/// Bounded for predictable layout time; exits early once nothing overlaps.
const CLUSTER_SEPARATION_PASSES: usize = 12;

/// How firmly a node is held toward its cluster anchor.
///
/// Strong enough that lobes separate and stay separated; loose enough that the
/// graph's own edges still shape what happens inside one. Tuned by looking at
/// the result, which is the only way this can be judged.
pub const CLUSTER_PULL: f64 = 0.5;

/// How many `::` segments of a package path define a cluster.
///
/// Depth 2 gives 86 groups over Architext's 306 modules -- lobes of roughly
/// forty functions, which is a readable size. Depth 1 collapses to the five
/// crates (too coarse to navigate), depth 3 fragments into 242 (barely
/// coarser than the modules themselves).
const CLUSTER_PKG_DEPTH: usize = 2;

/// The cluster each node belongs to, plus the cluster count.
///
/// Clusters are what turn the graph from one contiguous sphere into lobes.
/// Both tiers use the same key -- a package prefix -- so a module and the
/// functions inside it land in the same neighbourhood at either zoom level,
/// and moving between tiers does not rearrange the reader's mental map.
///
/// Node ORDER mirrors `build_graph` exactly (`modules.iter()` /
/// `functions.iter()`); the two are read together and must stay in step.
pub fn cluster_ids(cg: &CodeGraph, tier: Tier) -> (Vec<usize>, usize) {
    let prefix = |pkg: &str| -> String {
        pkg.split("::").take(CLUSTER_PKG_DEPTH).collect::<Vec<_>>().join("::")
    };
    let mut keys: Vec<String> = Vec::new();
    let empty_m = Vec::new();
    let modules = cg.modules.as_ref().unwrap_or(&empty_m);

    match tier {
        Tier::Modules => {
            for m in modules {
                keys.push(prefix(&m.pkg));
            }
        }
        Tier::Functions => {
            // Functions carry a symbol, not a package, so ownership comes from
            // the module that lists them. A function no module claims gets its
            // own key rather than being lumped into someone else's lobe.
            let mut owner: std::collections::HashMap<&str, String> =
                std::collections::HashMap::new();
            for m in modules {
                for fid in &m.function_ids {
                    owner.insert(fid.as_str(), prefix(&m.pkg));
                }
            }
            let empty_f = Vec::new();
            for f in cg.functions.as_ref().unwrap_or(&empty_f) {
                keys.push(owner.get(f.id.as_str()).cloned().unwrap_or_else(|| f.id.clone()));
            }
        }
    }

    // Stable numbering by first appearance, so the same graph always yields
    // the same cluster ids and therefore the same layout.
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut ids = Vec::with_capacity(keys.len());
    for k in &keys {
        let next = seen.len();
        ids.push(*seen.entry(k.as_str()).or_insert(next));
    }
    let count = seen.len();
    (ids, count)
}

/// Per-node anchor points, one per node, derived by laying the CLUSTERS out
/// against each other first.
///
/// This is the two-level part: clusters are settled as their own small graph
/// (86 nodes rather than 3,638), then every member is anchored to its
/// cluster's position. The member simulation still runs in full, so structure
/// inside a lobe is the real graph -- the anchor only decides which
/// neighbourhood it occupies.
pub fn cluster_anchors(
    clusters: &[usize],
    cluster_count: usize,
    edges: &[(usize, usize)],
    cfg: &ForceConfig,
) -> Vec<(f64, f64)> {
    if cluster_count <= 1 {
        return Vec::new();
    }
    // Aggregate member edges into cluster-to-cluster edges.
    let mut pairs: std::collections::BTreeSet<(usize, usize)> = Default::default();
    for &(a, b) in edges {
        let (ca, cb) = (clusters[a], clusters[b]);
        if ca != cb {
            pairs.insert((ca.min(cb), ca.max(cb)));
        }
    }
    let cluster_edges: Vec<(usize, usize)> = pairs.into_iter().collect();

    // Spread the clusters far enough apart that their members do not merge
    // back into one mass: the ideal separation scales with how many members
    // the biggest lobe has to hold.
    let mut sizes = vec![0usize; cluster_count];
    for &c in clusters {
        sizes[c] += 1;
    }
    // Space the clusters by the TYPICAL lobe, not the largest one. Sizing off
    // the biggest cluster spread 86 lobes across ~10,000 units, so each lobe
    // rendered as a dot in mostly empty space -- the separation was real and
    // useless. The mean keeps lobes large relative to the whole.
    let mean_size = (clusters.len() as f64 / cluster_count as f64).max(1.0);
    let cluster_cfg = ForceConfig {
        k: cfg.k * mean_size.sqrt().max(2.0),
        cluster_pull: 0.0,
        ..*cfg
    };
    let settled = crate::force_layout::simulate(
        cluster_count,
        &cluster_edges,
        LAYOUT_SEED ^ 0x5EED_C105,
        &cluster_cfg,
    );
    let mut centres = settled.positions;

    // Give every lobe the room its membership needs.
    //
    // Members repel each other against a central spring, so a lobe's natural
    // radius already grows as sqrt(members) -- the same law the seed circle
    // uses. The CLUSTER layout did not know that: it spaced all 86 centres
    // uniformly, so `architext_viewer::components` (hundreds of functions) got
    // the same allowance as a singleton. The big lobe swallowed its
    // neighbours and everything else scattered thin.
    //
    // Separating centres by the sum of their radii makes each territory
    // proportional to what it holds, which is the whole of the fix: no
    // constant can serve both a 400-member cluster and a 1-member one.
    let radius: Vec<f64> =
        sizes.iter().map(|&n| LOBE_RADIUS_K * (n as f64).sqrt().max(1.0)).collect();

    // Compact, then separate, repeatedly.
    //
    // Separation alone only enforces a MINIMUM distance -- nothing ever pulls
    // lobes back in, so the field stayed as wide as the force settle first
    // flung it and the graph read as sparse dots in empty space. Pulling every
    // centre toward the centroid and re-separating converges on the tightest
    // packing that still gives each lobe its room: compaction squeezes until
    // separation refuses, which is exactly where "tight" is.
    for _ in 0..CLUSTER_COMPACTION_PASSES {
        let (cx, cy) = (
            centres.iter().map(|c| c.0).sum::<f64>() / cluster_count as f64,
            centres.iter().map(|c| c.1).sum::<f64>() / cluster_count as f64,
        );
        for c in centres.iter_mut() {
            c.0 = cx + (c.0 - cx) * COMPACTION_STEP;
            c.1 = cy + (c.1 - cy) * COMPACTION_STEP;
        }
        for _ in 0..CLUSTER_SEPARATION_PASSES {
            let mut moved = false;
            for i in 0..cluster_count {
                for j in (i + 1)..cluster_count {
                    let (dx, dy) = (centres[j].0 - centres[i].0, centres[j].1 - centres[i].1);
                    let d = (dx * dx + dy * dy).sqrt();
                    let need = radius[i] + radius[j];
                    if d >= need {
                        continue;
                    }
                    let (ux, uy) = if d > 1e-9 { (dx / d, dy / d) } else { (1.0, 0.0) };
                    let push = (need - d) / 2.0;
                    centres[i].0 -= ux * push;
                    centres[i].1 -= uy * push;
                    centres[j].0 += ux * push;
                    centres[j].1 += uy * push;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }

    for _ in 0..CLUSTER_SEPARATION_PASSES {
        let mut moved = false;
        for i in 0..cluster_count {
            for j in (i + 1)..cluster_count {
                let (dx, dy) = (centres[j].0 - centres[i].0, centres[j].1 - centres[i].1);
                let d = (dx * dx + dy * dy).sqrt();
                let need = radius[i] + radius[j];
                if d >= need {
                    continue;
                }
                // Coincident centres have no direction to separate along, so
                // pick a deterministic one rather than dividing by zero.
                let (ux, uy) = if d > 1e-9 { (dx / d, dy / d) } else { (1.0, 0.0) };
                let push = (need - d) / 2.0;
                centres[i].0 -= ux * push;
                centres[i].1 -= uy * push;
                centres[j].0 += ux * push;
                centres[j].1 += uy * push;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }

    clusters.iter().map(|&c| centres[c]).collect()
}

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

    // Cluster membership travels with the model so every settle path gets it.
    let (clusters, cluster_count) = cluster_ids(cg, tier);

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
        clusters,
        cluster_count,
    }
}

/// The result of applying a [`FilterState`] (and, while an animation is
/// running, a [`Wavefront`]) to a graph: WHAT GETS UPLOADED. `nodes`/`edges`
/// hold FULL-graph indices. When a `Wavefront` narrowed the set, `edges` is
/// reordered lower-depth-endpoint-first (see [`Wavefront`]'s doc) — callers
/// that don't pass one keep the original call-direction order untouched.
///
/// Two DISTINCT full-graph node masks, deliberately kept separate rather than
/// folded into one `visible` (the pre-fix representation, which conflated
/// them and made every node but the current wavefront's ~handful
/// unclickable):
/// - `visible` — filter AND wavefront. This is what gets DRAWN: an
///   un-reached node/edge is culled from the render, not merely faded (the
///   "17.5k nodes stay legible" requirement). Use this for anything about
///   what is currently on screen.
/// - `filter_visible` — filter ONLY, never narrowed by an in-progress
///   wavefront. A node the user asked to hide (via the reachability/edge-kind
///   checkboxes) is correctly absent here; a node the wavefront simply
///   hasn't reached YET is still present. Use this for anything about
///   whether a node is a legitimate interaction target: hit-testing (a click
///   must be able to reach an animation-culled node — it is real and
///   on-screen-adjacent, just not yet drawn) and selection LIFETIME (a
///   selection must survive the wavefront advancing past it; only a filter
///   change may legitimately drop it — see `ViewState::recompute_cull`).
pub struct Cull {
    pub visible: Vec<bool>,
    pub filter_visible: Vec<bool>,
    pub nodes: Vec<usize>,
    pub edges: Vec<(usize, usize, bool)>,
}

/// The animation-wavefront restriction layered on top of a [`FilterState`]
/// cull — the "cull un-reached nodes, don't just fade them" requirement.
/// `depth`/`current` are the same full-graph depth field and current-hop
/// depth `ViewState` already tracks; `growth` is `false` under
/// `prefers-reduced-motion` and collapses the in-flight (partially drawn)
/// band into a cull too, so a reduced-motion run reveals whole hops at once
/// with no partially-grown edge ever uploaded.
#[derive(Clone, Copy)]
pub struct Wavefront<'a> {
    pub depth: &'a [i32],
    pub current: i32,
    pub growth: bool,
}

/// Compute the cull sets. A node survives if the filter shows its class; an
/// edge survives if BOTH endpoints survive and the filter shows its kind.
///
/// With `wave: Some(_)`, nodes are further restricted to ones the wavefront
/// has FULLY reached (`0 <= depth <= current`) — an unreached node is not
/// uploaded at all, never merely faded. Edges get one extra allowance beyond
/// that: the "in-flight" edge for the CURRENT hop (one endpoint already
/// reached, the other exactly one hop ahead) survives too, reordered
/// lower-depth-endpoint-first so `edge_state`'s `progress` value and the GPU
/// shader agree on which end the line grows FROM — regardless of the call
/// direction or which BFS direction produced the depths. Anything further
/// ahead than that is culled outright.
pub fn cull(graph: &GraphModel, filter: &FilterState, wave: Option<Wavefront>) -> Cull {
    let filter_visible = filter.visible_nodes(
        &graph.prod_reachable,
        &graph.dead,
        &graph.test_only,
        &graph.generated,
    );
    let visible: Vec<bool> = match &wave {
        None => filter_visible.clone(),
        Some(w) => filter_visible
            .iter()
            .enumerate()
            .map(|(i, &fv)| fv && w.depth[i] >= 0 && w.depth[i] <= w.current)
            .collect(),
    };
    let nodes: Vec<usize> = (0..graph.node_count()).filter(|&i| visible[i]).collect();

    let mut edges: Vec<(usize, usize, bool)> = Vec::new();
    for &(a, b, dynamic) in &graph.directed_edges {
        if !(filter_visible[a] && filter_visible[b] && filter.edge_visible(dynamic)) {
            continue;
        }
        match &wave {
            None => edges.push((a, b, dynamic)),
            Some(w) => {
                let (da, db) = (w.depth[a], w.depth[b]);
                if da < 0 || db < 0 {
                    continue; // never reached by the BFS at all
                }
                let (lo_i, hi_i, lo_d, hi_d) = if da <= db { (a, b, da, db) } else { (b, a, db, da) };
                let in_flight = hi_d == w.current + 1 && lo_d <= w.current && w.growth;
                if hi_d <= w.current || in_flight {
                    edges.push((lo_i, hi_i, dynamic));
                }
            }
        }
    }
    Cull { visible, filter_visible: filter_visible.clone(), nodes, edges }
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

/// Per-uploaded-node `[alpha, glow, colorMix, hopAge]` — folds the animation
/// wavefront and the selection highlight into one pass so they compose
/// instead of fighting. (No filter fold: culled nodes are not uploaded at
/// all — that is the whole point of the cull.)
///
/// When `anim_mode != Off`, `c` is assumed to already be a
/// [`cull`]-with-[`Wavefront`] result: every uploaded node satisfies
/// `0 <= anim_depth[g] <= anim_current` by construction (an unreached node
/// is never in `c.nodes` at all — see `cull`'s doc — so there is no "fade
/// the rest of the haze" branch here to fight with the real cull upstream).
///
/// Slot 3 (formerly always-zero padding) is `hopAge` — the comet-trail
/// brightness decay input `gl/shaders.rs`'s `NODE_VS` consumes (see
/// `hop_age`'s doc). While an animation is running AND motion is enabled,
/// alpha/colorMix are uploaded at their PEAK (1.0/1.0) for every reached
/// node — the two-bucket "just arrived vs. flat trail" split this replaced
/// is now the shader's job, driven continuously by `hopAge`, not a discrete
/// CPU-computed level. `reduced_motion` (and the Off/selection paths) upload
/// `0.0`, which is `decay_brightness`'s identity input (`age <= 0` → no
/// dimming) — those paths are therefore bit-for-bit unchanged. `glow` stays
/// a discrete "just arrived this hop" flash, untouched by decay — it is a
/// brief selection-style cue, not part of the trail the maintainer asked to
/// see fade.
#[allow(clippy::too_many_arguments)] // same precedent as sync_plan.rs / diagram/sequence.rs
pub fn node_state(
    graph: &GraphModel,
    c: &Cull,
    selected: Option<usize>,
    anim_mode: AnimMode,
    anim_depth: &[i32],
    anim_current: i32,
    hop_progress: f32,
    reduced_motion: bool,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(c.nodes.len() * 4);
    for &g in &c.nodes {
        let (mut alpha, mut glow, mut mix, mut age) = (0.85_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        if anim_mode != AnimMode::Off {
            alpha = 1.0;
            mix = 1.0;
            if anim_depth[g] == anim_current {
                glow = 0.9;
            }
            if !reduced_motion {
                age = hop_age(anim_depth[g], anim_current, hop_progress);
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
        out.extend_from_slice(&[alpha, glow, mix, age]);
    }
    out
}

/// Per-uploaded-edge `[alpha, colorMix, progress, hopAge]`. The base alpha
/// drops at scale (>4000 visible edges) so a dense tier stays readable —
/// same threshold as the spike. `progress` (slot 2, previously always-zero
/// padding — see `gl/shaders.rs`'s `EDGE_VS`) is the GPU interpolation
/// factor the vertex shader draws the line to, `mix(loDepthEnd, hiDepthEnd,
/// progress)`: `1.0` for every edge outside an active animation (fully
/// drawn — unchanged visual for the Off/selection paths) and for any edge
/// already fully behind the wavefront, and the live `hop_progress` for the
/// single in-flight band `cull`-with-[`Wavefront`] admits (one endpoint at
/// `anim_current`, the other one hop ahead) — the "draw a line from one node
/// to the next" requirement. `c.edges` is assumed lower-depth-endpoint-first
/// (guaranteed by `cull` whenever a `Wavefront` was passed), so `a` is always
/// the line's grow-FROM end while animating.
///
/// Slot 3 (`hopAge`) is the comet-trail brightness decay input
/// `gl/shaders.rs`'s `EDGE_VS` consumes — see `node_state`'s doc for the
/// same PEAK-upload-plus-GPU-decay rationale, mirrored here: alpha/colorMix
/// upload at their peak (0.9/1.0) for every reached-or-in-flight edge while
/// motion is enabled, replacing the old three-way discrete alpha split with
/// one continuous shader-side curve keyed on `hop_age(hi_depth, ...)` (the
/// edge's DEEPER endpoint — the one that determines when it stops growing).
/// An in-flight edge's age is negative (see `hop_age`'s doc), so it gets the
/// same brightest treatment as the instant it fully arrives — no special
/// case needed alongside the `progress` handling above.
pub fn edge_state(
    c: &Cull,
    selected: Option<usize>,
    anim_mode: AnimMode,
    anim_depth: &[i32],
    anim_current: i32,
    hop_progress: f32,
    reduced_motion: bool,
) -> Vec<f32> {
    let base = if c.edges.len() > 4000 { 0.05 } else { 0.28 };
    let mut out = Vec::with_capacity(c.edges.len() * 4);
    for &(a, b, _) in &c.edges {
        let (mut alpha, mut mix, mut progress, mut age) = (base, 0.0_f32, 1.0_f32, 0.0_f32);
        if anim_mode != AnimMode::Off {
            let hi_depth = anim_depth[a].max(anim_depth[b]);
            alpha = 0.9;
            mix = 1.0;
            if hi_depth == anim_current + 1 {
                // In-flight: the one hop this wavefront is currently drawing
                // toward. `progress` stays the live hop fraction; every other
                // branch leaves it at the 1.0 default (fully drawn).
                progress = hop_progress;
            }
            if !reduced_motion {
                age = hop_age(hi_depth, anim_current, hop_progress);
            }
        } else if let Some(s) = selected {
            if a == s || b == s {
                alpha = 0.9;
                mix = 1.0;
            } else {
                alpha = SELECT_FADE_ALPHA * 0.5;
            }
        }
        out.extend_from_slice(&[alpha, mix, progress, age]);
    }
    out
}

/// Pure timing step for the wavefront (called once per RAF frame while
/// playing): advances `hop_progress` by `dt_ms / hop_duration_ms`, rolling
/// over into `current_depth` (clamped to `max_depth`) whenever progress
/// reaches 1.0 — possibly several hops at once if `dt_ms` is unusually large
/// (e.g. a backgrounded tab's RAF loop resuming after a stall; see
/// `STALL_RESUME_THRESHOLD_MS` in `code_graph_view.rs`). Returns
/// `(new_depth, new_hop_progress, finished)`; once `finished` is true the
/// wavefront has reached `max_depth` and `hop_progress` is pinned at 0 —
/// there is no next hop to draw a partial line toward, so playback should
/// stop rather than free-spin.
pub fn advance_hop(
    current_depth: i32,
    hop_progress: f32,
    max_depth: i32,
    dt_ms: f64,
    hop_duration_ms: f64,
) -> (i32, f32, bool) {
    if current_depth >= max_depth || hop_duration_ms <= 0.0 {
        return (current_depth.max(max_depth), 0.0, true);
    }
    let mut depth = current_depth;
    let mut progress = hop_progress + (dt_ms / hop_duration_ms) as f32;
    while progress >= 1.0 && depth < max_depth {
        progress -= 1.0;
        depth += 1;
    }
    if depth >= max_depth {
        (depth, 0.0, true)
    } else {
        (depth, progress.max(0.0), false)
    }
}

/// The 10th/90th-percentile bounding box of `positions`, on each axis
/// independently (NOT distance-from-origin — see `fit_camera`'s doc for why
/// that distinction matters) — a handful of low-degree outliers sit at the
/// gravity equilibrium ring and must not dictate the framing alone
/// (spike-proven). Degenerates gracefully: empty input collapses to a
/// zero-width box at the origin; a single point collapses to a zero-width
/// box centred on it — `fit_camera` widens either to a 1-unit minimum so
/// neither divides by zero.
fn robust_bounds(positions: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let bound = |mut vals: Vec<f32>| -> (f32, f32) {
        if vals.is_empty() {
            return (0.0, 0.0);
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let lo_idx = ((vals.len() as f32 - 1.0) * 0.10).round().max(0.0) as usize;
        let hi_idx = ((vals.len() as f32 - 1.0) * 0.90).round().max(0.0) as usize;
        (vals[lo_idx], vals[hi_idx])
    };
    let (lo_x, hi_x) = bound(positions.iter().map(|(x, _)| *x).collect());
    let (lo_y, hi_y) = bound(positions.iter().map(|(_, y)| *y).collect());
    (lo_x, hi_x, lo_y, hi_y)
}

/// The centre of the robust bounding box (see `robust_bounds`) — the BOX
/// centre. Historically what `fit_camera` panned to; kept (and still
/// public) as a reference statistic for tests to compare against, now that
/// `fit_camera`'s pan target is `density_centre` instead (see its doc and
/// `fit_camera`'s for why the two are deliberately different statistics).
pub fn centroid(positions: &[(f32, f32)]) -> (f32, f32) {
    let (lo_x, hi_x, lo_y, hi_y) = robust_bounds(positions);
    ((lo_x + hi_x) / 2.0, (lo_y + hi_y) / 2.0)
}

/// The density-weighted centre — the median of x and of y independently,
/// over the FULL (untrimmed) position set. Historically `fit_camera`'s PAN
/// target; kept (and still `pub`) as the UNWEIGHTED reference statistic —
/// `fit_camera` now pans by `weighted_centre` instead (see its doc: this
/// function counts every node equally regardless of how much of it is
/// actually drawn, which undercounts a hub's rendered ink). Tests compare
/// the two directly to prove they diverge once node size varies.
///
/// Chosen over a mean/centroid: on a hub-and-ring topology (a dense hub
/// plus a large one-sided ring of low-degree, mostly-disconnected modules
/// that settle onto the gravity-equilibrium ring — see `robust_bounds`'
/// doc) a mean is dragged toward the ring by every one of its nodes' exact
/// distance from the mass, same as the box centre (`centroid`) — no
/// improvement. A median resists this by construction: as long as the ring
/// is under ~50% of the node count, no amount of one-sided distance moves
/// the median off the hub, whereas the box's 10th/90th-percentile bounds
/// (and a mean) shift with any lopsided minority. Degenerates the same way
/// `robust_bounds` does: empty input collapses to the origin, a single
/// point collapses to itself.
pub fn density_centre(positions: &[(f32, f32)]) -> (f32, f32) {
    let median = |mut vals: Vec<f32>| -> f32 {
        if vals.is_empty() {
            return 0.0;
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((vals.len() as f32 - 1.0) * 0.5).round().max(0.0) as usize;
        vals[idx]
    };
    let mx = median(positions.iter().map(|(x, _)| *x).collect());
    let my = median(positions.iter().map(|(_, y)| *y).collect());
    (mx, my)
}

/// A single axis's weighted median: sort `values`, then walk that order
/// accumulating `weights` until the point where each element's own
/// half-weight midpoint (cumulative weight before it, plus half its own
/// weight) is closest to the half-of-total-weight mark. Ties keep the
/// LATER element — matching `f32::round`'s round-half-up-away-from-zero —
/// so this reduces EXACTLY to the plain rank median (`density_centre`'s
/// helper) when every weight is equal; a test asserts that invariant
/// directly rather than trusting the algebra. "Tie" is judged within a
/// small epsilon of `total`, not bit-exact equality: an even element count
/// with truly equal weights lands EXACTLY on the half-weight mark from two
/// adjacent ranks, and float summation drift across hundreds of additions
/// (unavoidable — weights are accumulated, not recomputed from scratch
/// each step) can otherwise tip that exact tie to the wrong side. `values`
/// and `weights` must be index-aligned, the same contract `GraphModel`'s
/// parallel vecs use.
fn weighted_median(values: &[f32], weights: &[f32]) -> f32 {
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap());
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        // No positive weight to accumulate toward — shouldn't happen with
        // real radii (clamped >= 3.0), but fall back to the plain rank
        // median rather than divide by zero.
        let mid = ((n as f32 - 1.0) * 0.5).round().max(0.0) as usize;
        return values[order[mid]];
    }
    let target = total / 2.0;
    let tie_epsilon = total * 1e-4;
    let mut cumulative = 0.0_f32;
    let mut best_rank = 0;
    let mut best_dist = f32::INFINITY;
    for (rank, &i) in order.iter().enumerate() {
        let midpoint = cumulative + weights[i] / 2.0;
        let dist = (midpoint - target).abs();
        if dist <= best_dist + tie_epsilon {
            best_dist = dist.min(best_dist);
            best_rank = rank;
        }
        cumulative += weights[i];
    }
    values[order[best_rank]]
}

/// The rendered-mass-weighted centre — `weighted_median` applied to x and y
/// independently, weighted by each node's `GraphModel::radius` (the same
/// degree-derived quantity `radius_for` computes — this deliberately does
/// NOT invent a second notion of "how big is this node"). `fit_camera`'s
/// actual PAN target.
///
/// Weighting fixes what plain `density_centre` leaves on the table: node
/// radius scales with degree (`radius_for`, sqrt-scaled and clamped to
/// 3..22), so a hub draws far more ink than a peripheral node — median NODE
/// POSITION and centre of rendered INK are different points whenever sizes
/// vary by an order of magnitude, which they do on this codebase's real
/// corpus (dense hub + ring of near-isolates; measured directly as a ~4%
/// pixel-centre-of-mass offset in a real browser). A degree-WEIGHTED MEAN
/// was rejected for the same reason a plain mean was rejected for
/// `density_centre`: it lets a few big nodes, wherever they sit, drag the
/// centre by their exact distance from the mass — the box centre's
/// failure mode all over again. A weighted MEDIAN keeps the median's
/// robustness (see `density_centre`'s doc) while making per-node influence
/// proportional to `radius`, which is itself clamped — so no single node,
/// however high its degree, can dominate the crossing point unboundedly.
/// Degenerates the same way `density_centre` does: empty input collapses to
/// the origin, a single point collapses to itself.
pub fn weighted_centre(positions: &[(f32, f32)], weights: &[f32]) -> (f32, f32) {
    let xs: Vec<f32> = positions.iter().map(|(x, _)| *x).collect();
    let ys: Vec<f32> = positions.iter().map(|(_, y)| *y).collect();
    (weighted_median(&xs, weights), weighted_median(&ys, weights))
}

/// Fit-to-viewport camera (zoom AND pan) for a fresh layout. Replaces the
/// old `fit_zoom`, which measured the 90th percentile of `|x|`/`|y|` —
/// distance FROM THE ORIGIN — and so silently assumed the layout was
/// centred there; any drift of the actual centroid off-origin (asymmetric
/// graphs, gravity-equilibrium drift) showed up as off-centre framing even
/// though the zoom level was reasonable.
///
/// Zoom fits the robust bounding box (see `robust_bounds`) into `w x h`
/// with the same 1.1x margin the old `fit_zoom` used, clamped to the same
/// `0.02..3.0` range — outliers up to the 90th percentile stay on screen.
/// This statistic is UNCHANGED by the pan fix below; outliers must remain
/// framed, they simply stop dictating the CENTRE. `weights` plays NO part
/// in the zoom computation — sizing the frame and centring the mass are
/// different jobs, and conflating them is exactly what produced the
/// original bug.
///
/// Pan places the rendered-mass-weighted centre (see `weighted_centre`) at
/// the viewport centre — deliberately a DIFFERENT statistic from the box
/// the zoom above measures, and now also different from the plain
/// `density_centre` this replaced as the pan target: median NODE POSITION
/// undercounts a hub, which draws far more ink than a same-position
/// peripheral node once radius scales with degree (measured directly as a
/// ~4% pixel-centre-of-mass offset in a real browser, hub-and-ring corpus).
/// `weights` should be the same per-node quantity `GraphModel::radius`
/// already carries — see `weighted_centre`'s doc for why reusing it (not a
/// second size notion) matters, and why a weighted MEDIAN, not a weighted
/// mean, keeps this robust against the gravity-equilibrium ring
/// `robust_bounds`' doc describes. Centring on the mass (pan) and framing
/// the extent (zoom) stay different jobs — do not unify them back into one
/// statistic; conflating them is exactly what produced the original bug.
pub fn fit_camera(positions: &[(f32, f32)], weights: &[f32], w: f32, h: f32) -> (f32, f32, f32) {
    let (lo_x, hi_x, lo_y, hi_y) = robust_bounds(positions);
    let box_w = (hi_x - lo_x).max(1.0);
    let box_h = (hi_y - lo_y).max(1.0);
    let zoom = (w / (box_w * 1.1)).min(h / (box_h * 1.1)).clamp(0.02, 3.0);
    let (cx, cy) = weighted_centre(positions, weights);
    let pan_x = w / 2.0 - cx * zoom;
    let pan_y = h / 2.0 - cy * zoom;
    (zoom, pan_x, pan_y)
}

/// The imperative per-frame state. Kept out of Leptos signals by the view —
/// a redraw is GPU submission work, not a vdom diff; signals only MIRROR the
/// few display facts (counts, selected label, depth) for the chrome.
pub struct ViewState {
    pub graph: GraphModel,
    /// Full-graph layout positions (stable across filter changes).
    pub positions: Vec<(f32, f32)>,
    /// Hit-test tree over the FULL graph (filter-culled hits are rejected via
    /// `cull.filter_visible` — never `cull.visible`, which also narrows to
    /// the current animation wavefront and would make every node the
    /// wavefront hasn't reached yet unclickable; see `Cull`'s doc — keeping
    /// the tree — and the layout — filter-stable).
    pub tree: QuadTree,
    /// True while the progressive layout (Task 5) is still ticking: the
    /// painted positions refresh every frame but `tree` still covers the
    /// PRE-settle positions, so click hit-testing is gated off until the
    /// layout settles and the tree is rebuilt over the final positions.
    pub layout_settling: bool,
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
    /// Progress (0..1) through the hop from `anim_current_depth` toward
    /// `anim_current_depth + 1` — the GPU-shader interpolation factor for the
    /// one in-flight edge band `cull` admits while playing. Always `0.0`
    /// outside an active hop (mode off, paused-at-a-clean-boundary via
    /// step/scrub, or finished).
    pub anim_hop_progress: f32,
    /// `prefers-reduced-motion: reduce`, read once when the tier's layout is
    /// (re)seeded (`code_graph_view.rs`) — gates both the pacing constant
    /// `advance_animation` uses and whether `cull` admits the in-flight
    /// partial-progress band at all (see `Wavefront::growth`).
    pub reduced_motion: bool,
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
        reduced_motion: bool,
    ) -> Self {
        let filter = FilterState::default();
        let cull = cull(&graph, &filter, None);
        Self {
            graph,
            positions,
            tree,
            layout_settling: false,
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
            anim_hop_progress: 0.0,
            reduced_motion,
        }
    }

    /// Re-apply the current filter (after a checkbox flip) AND, while an
    /// animation is running, the current wavefront depth/progress — the two
    /// restrictions compose for RENDERING (a filter change while
    /// mid-animation must not resurrect edges the wavefront hasn't reached).
    ///
    /// Selection lifetime, though, is judged against `filter_visible` alone,
    /// never the wavefront-narrowed `visible`: the wavefront advancing past
    /// the selected node must NOT silently drop the selection (it is still a
    /// real, filter-visible node — merely not drawn as reached yet), only a
    /// filter change that genuinely hides the node may clear it.
    pub fn recompute_cull(&mut self) {
        let wave = (self.anim_mode != AnimMode::Off).then_some(Wavefront {
            depth: &self.anim_depth,
            current: self.anim_current_depth,
            growth: !self.reduced_motion,
        });
        self.cull = cull(&self.graph, &self.filter, wave);
        if let Some(s) = self.selected {
            if !self.cull.filter_visible[s] {
                self.selected = None;
            }
        }
    }

    /// Re-run the BFS wavefront for the current animation mode (seeds: the
    /// roots, or the selected node for Outbound/Inbound), reset to a clean
    /// depth-0 boundary, and re-cull to match (mode Off drops the wavefront
    /// restriction entirely, back to filter-only).
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
        self.anim_hop_progress = 0.0;
        self.recompute_cull();
    }

    /// Jump the wavefront straight to `depth` (step ⏮/⏭ or the scrub slider)
    /// — always a CLEAN boundary: `anim_hop_progress` resets to 0, so no
    /// edge is left mid-growth. Re-culls to match (the visible node/edge set
    /// changes at the new depth), same as a mode/selection reseed.
    pub fn set_anim_depth(&mut self, depth: i32) {
        self.anim_current_depth = depth.clamp(0, self.anim_max_depth);
        self.anim_hop_progress = 0.0;
        self.recompute_cull();
    }

    /// Advance the wavefront by `dt_ms` of playback time (one RAF frame while
    /// playing) — the pure timing/pacing logic lives in [`advance_hop`], gated
    /// by `reduced_motion` for which cadence constant applies. Returns
    /// `(boundary_crossed, finished)`: `boundary_crossed` tells the caller
    /// whether the node/edge SET changed (a `recompute_cull` ran, so a full
    /// re-upload of the STATIC buffers is needed) or only `anim_hop_progress`
    /// moved (a DYNAMIC-only re-upload suffices); `finished` tells the caller
    /// to stop playback.
    pub fn advance_animation(&mut self, dt_ms: f64) -> (bool, bool) {
        let hop_ms =
            if self.reduced_motion { REDUCED_MOTION_HOP_DURATION_MS } else { HOP_DURATION_MS };
        let (depth, progress, finished) =
            advance_hop(self.anim_current_depth, self.anim_hop_progress, self.anim_max_depth, dt_ms, hop_ms);
        let boundary_crossed = depth != self.anim_current_depth;
        self.anim_current_depth = depth;
        self.anim_hop_progress = progress;
        if boundary_crossed {
            self.recompute_cull();
        }
        (boundary_crossed, finished)
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
            self.anim_hop_progress,
            self.reduced_motion,
        )
    }

    pub fn edge_state(&self) -> Vec<f32> {
        edge_state(
            &self.cull,
            self.selected,
            self.anim_mode,
            &self.anim_depth,
            self.anim_current_depth,
            self.anim_hop_progress,
            self.reduced_motion,
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
        let c = cull(&g, &FilterState::default(), None);
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
        assert_eq!(cull(&g, &f, None).edges.len(), 3, "everything shown");
        f.show_dynamic = false;
        let c = cull(&g, &f, None);
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
        let c = cull(&g, &FilterState::default(), None);
        let s = node_state(&g, &c, None, AnimMode::Off, &[], 0, 0.0, false);
        assert_eq!(s.len(), c.nodes.len() * 4);
        assert_eq!(&s[0..4], &[0.85, 0.0, 0.0, 0.0], "base state, no selection/anim");
    }

    #[test]
    fn node_state_selection_highlights_self_and_neighbours() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default(), None);
        // Select handler (graph idx 1); its uploaded neighbour is main (0).
        let s = node_state(&g, &c, Some(1), AnimMode::Off, &[], 0, 0.0, false);
        let slot_main = &s[0..4];
        let slot_handler = &s[4..8];
        assert_eq!(slot_handler, &[1.0, 0.8, 1.0, 0.0], "selected: full alpha + glow");
        assert_eq!(slot_main, &[1.0, 0.0, 1.0, 0.0], "neighbour: full alpha, no glow");
    }

    #[test]
    fn wavefront_cull_admits_reached_and_in_flight_only_and_recedes_never_uploads() {
        // THE required behaviour change (maintainer: "what happened to
        // culling the background items"): un-reached nodes/edges are not
        // uploaded at all while an animation runs — no fade branch to check.
        let g = build_graph(&fixture(), Tier::Functions);
        let depth = depth_field(4, &g.index.bfs(Direction::Outbound, &[0])); // main=0,handler=1,helper=2,tests=-1
        let base = cull(&g, &FilterState::default(), None);

        // At current_depth 0: only main (depth 0) is reached; handler (depth
        // 1) is the in-flight target this hop, so main→handler survives as
        // the growing edge, but handler itself is not uploaded yet.
        let c0 = cull(&g, &FilterState::default(), Some(Wavefront { depth: &depth, current: 0, growth: true }));
        assert_eq!(c0.nodes, vec![0], "handler hasn't fully arrived yet — not uploaded");
        assert_eq!(c0.edges, vec![(0, 1, false)], "the in-flight edge is admitted, lo-depth-first");

        // At current_depth 1: main and handler are both reached; the dead
        // helper (depth 2, filtered out by the default reachability filter
        // regardless) stays absent, and main→handler is now fully behind the
        // wavefront (still present, no longer "in-flight").
        let c1 = cull(&g, &FilterState::default(), Some(Wavefront { depth: &depth, current: 1, growth: true }));
        assert_eq!(c1.nodes, vec![0, 1]);
        assert_eq!(c1.edges, vec![(0, 1, false)]);

        // With growth disabled (prefers-reduced-motion): the in-flight band
        // collapses into a cull too — at current_depth 0 NEITHER node 1 nor
        // the edge toward it appears; base filter cull is untouched.
        let reduced =
            cull(&g, &FilterState::default(), Some(Wavefront { depth: &depth, current: 0, growth: false }));
        assert_eq!(reduced.nodes, vec![0]);
        assert!(reduced.edges.is_empty(), "no partial edge without growth");
        assert_eq!(base.nodes, vec![0, 1], "the filter-only cull is unaffected by any of this");
    }

    #[test]
    fn edge_state_in_flight_progress_tracks_hop_progress_others_stay_full() {
        let g = build_graph(&fixture(), Tier::Functions);
        let depth = depth_field(4, &g.index.bfs(Direction::Outbound, &[0]));
        let c = cull(&g, &FilterState::default(), Some(Wavefront { depth: &depth, current: 0, growth: true }));
        // Only edge (0,1) is uploaded (see the cull test above) and it is
        // in-flight at current_depth 0 — progress must equal hop_progress,
        // not the Off-path default of 1.0.
        let s = edge_state(&c, None, AnimMode::Roots, &depth, 0, 0.37, false);
        assert_eq!(s.len(), 4);
        assert_eq!(s[2], 0.37, "in-flight edge progress mirrors hop_progress exactly");

        // Once main→handler is fully behind the wavefront (current_depth 1),
        // its progress is pinned at 1.0 regardless of hop_progress.
        let c1 = cull(&g, &FilterState::default(), Some(Wavefront { depth: &depth, current: 1, growth: true }));
        let s1 = edge_state(&c1, None, AnimMode::Roots, &depth, 1, 0.9, false);
        assert_eq!(s1[2], 1.0, "a fully-reached edge is always fully drawn");
    }

    #[test]
    fn edge_state_interleave_and_selection_accent() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default(), None);
        let s = edge_state(&c, None, AnimMode::Off, &[], 0, 0.0, false);
        assert_eq!(s.len(), c.edges.len() * 4, "4 f32 per uploaded edge");
        assert_eq!(&s[0..4], &[0.28, 0.0, 1.0, 0.0], "small-graph base alpha, fully drawn (Off)");
        let s = edge_state(&c, Some(1), AnimMode::Off, &[], 0, 0.0, false);
        assert_eq!(&s[0..4], &[0.9, 1.0, 1.0, 0.0], "edge touching the selection accents, fully drawn");
    }

    #[test]
    fn static_interleaves_follow_the_upload_layout() {
        let g = build_graph(&fixture(), Tier::Functions);
        let c = cull(&g, &FilterState::default(), None);
        let positions = vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (30.0, 0.0)];
        let (npr, ee) = static_interleaves(&g, &c, &positions);
        assert_eq!(npr.len(), c.nodes.len() * 3, "[x, y, radius] per node");
        assert_eq!(ee.len(), c.edges.len() * 4, "[fx, fy, tx, ty] per edge");
        assert_eq!(&npr[0..3], &[0.0, 0.0, g.radius[0]]);
        assert_eq!(&ee[0..4], &[0.0, 0.0, 10.0, 0.0], "main→handler endpoints");
    }

    #[test]
    fn fit_camera_is_bounded_and_handles_empty_input() {
        // Uniform weights here: this test is about the zoom/pan MECHANICS
        // (clamping, empty input), not weighting, and equal weights make
        // `weighted_centre` coincide exactly with the old `density_centre`
        // pan target (see `weighted_centre_matches_density_centre_...`).
        let positions = vec![(0.0, 0.0), (100.0, 50.0), (-100.0, -50.0)];
        let weights = vec![1.0; positions.len()];
        let (z, _, _) = fit_camera(&positions, &weights, 1600.0, 1000.0);
        assert!((0.02..=3.0).contains(&z), "zoom clamped, got {z}");
        // Preserved from `fit_zoom`: an empty layout collapses to a unit
        // box, which clamps zoom to the 3.0 max — still the least
        // surprising choice for "nothing to frame" (matches the old
        // behaviour exactly). Pan for an empty/degenerate box centres the
        // (zero) content at the viewport centre rather than drifting.
        let (z_empty, pan_x, pan_y) = fit_camera(&[], &[], 1600.0, 1000.0);
        assert_eq!(z_empty, 3.0, "empty layout still clamps to max zoom");
        assert_eq!((pan_x, pan_y), (800.0, 500.0), "empty layout centres the viewport");
    }

    /// WHY: fit must frame the content wherever it actually sits. The old
    /// `fit_zoom` measured |x|,|y| from the origin, so a layout whose
    /// centroid drifted (gravity equilibrium, asymmetric graphs) framed
    /// off-centre even though the zoom level was reasonable.
    #[test]
    fn fit_camera_centres_on_content_not_the_origin() {
        // A cluster deliberately far from the origin.
        let positions: Vec<(f32, f32)> =
            (0..1000).map(|i| (1000.0 + (i % 30) as f32, 500.0 + (i / 30) as f32)).collect();
        let weights = vec![1.0; positions.len()]; // uniform: not a weighting test
        let (zoom, pan_x, pan_y) = fit_camera(&positions, &weights, 1600.0, 1000.0);
        assert!(zoom > 0.0 && zoom.is_finite());
        // The content centroid must land at the viewport centre.
        let (cx, cy) = centroid(&positions);
        let screen_x = cx * zoom + pan_x;
        let screen_y = cy * zoom + pan_y;
        assert!((screen_x - 800.0).abs() < 1.0, "content centre x at {screen_x}, want 800");
        assert!((screen_y - 500.0).abs() < 1.0, "content centre y at {screen_y}, want 500");
    }

    #[test]
    fn fit_camera_handles_empty_and_degenerate_input() {
        let (z, _, _) = fit_camera(&[], &[], 1600.0, 1000.0);
        assert!(z.is_finite() && z > 0.0);
        let single = vec![(42.0, 42.0)];
        let weight = vec![radius_for(9)];
        let (z2, _, _) = fit_camera(&single, &weight, 1600.0, 1000.0);
        assert!(z2.is_finite() && z2 > 0.0, "a single node must not divide by zero");
    }

    #[test]
    fn weighted_centre_handles_empty_and_degenerate_input() {
        let (wx, wy) = weighted_centre(&[], &[]);
        assert!(wx.is_finite() && wy.is_finite(), "empty input must not divide by zero or NaN");
        assert_eq!((wx, wy), (0.0, 0.0), "empty input collapses to the origin, same as density_centre");

        let single = [(42.0, -7.0)];
        let weight = [radius_for(9)];
        let (wx2, wy2) = weighted_centre(&single, &weight);
        assert_eq!((wx2, wy2), (42.0, -7.0), "a single node's weighted median is itself");
    }

    /// With all degrees equal, every node has the same radius/weight — the
    /// weighted median must reduce EXACTLY to the plain rank median
    /// (`density_centre`), not just approximately. This is the invariant
    /// `weighted_median`'s doc claims algebraically; this test proves it.
    #[test]
    fn weighted_centre_matches_density_centre_when_weights_are_equal() {
        let positions = hub_and_ring_positions();
        // An arbitrary non-1.0 constant — proves it's equal-weight
        // invariance, not a coincidence of weight == 1.0.
        let uniform = vec![radius_for(7); positions.len()];
        assert_eq!(
            weighted_centre(&positions, &uniform),
            density_centre(&positions),
            "equal weights must reduce exactly to the unweighted median"
        );

        // Odd-length input too (even/odd rank-median tie-break differs).
        let odd_positions: Vec<(f32, f32)> = positions[..999].to_vec();
        let odd_weights = vec![radius_for(3); odd_positions.len()];
        assert_eq!(
            weighted_centre(&odd_positions, &odd_weights),
            density_centre(&odd_positions),
            "odd-length equal weights must also match exactly"
        );
    }

    /// A handful of far-flung outliers (the gravity-equilibrium ring, per
    /// `robust_bounds`' doc) sit alongside a tight, ≥1000-node cluster.
    /// Neither the zoom nor the pan may be dictated by the outliers — both
    /// must still frame the cluster.
    #[test]
    fn fit_camera_outliers_dont_dictate_the_framing() {
        let mut positions: Vec<(f32, f32)> =
            (0..1000).map(|i| ((i % 40) as f32 - 20.0, (i / 40) as f32 - 12.0)).collect();
        // <1% of the node count — enough to previously wreck the fit, not
        // enough to survive the 90th-percentile trim.
        for k in 0..5 {
            positions.push((5000.0 + k as f32, -5000.0 - k as f32));
        }
        let weights = vec![1.0; positions.len()]; // uniform: not a weighting test
        let (zoom, pan_x, pan_y) = fit_camera(&positions, &weights, 1600.0, 1000.0);
        // The cluster spans roughly 40x24 units, which fits well past the
        // 3.0 zoom ceiling — so a trimmed box should clamp at the max. If
        // the outliers dictated the box instead, zoom would collapse to
        // ~0.16 (fitting a ~10,000-unit span) — nowhere near the clamp.
        assert_eq!(zoom, 3.0, "outliers must not crush the zoom below the clamp max, got {zoom}");
        let (cx, cy) = centroid(&positions);
        let screen_x = cx * zoom + pan_x;
        let screen_y = cy * zoom + pan_y;
        assert!((screen_x - 800.0).abs() < 1.0, "cluster centre x at {screen_x}, want 800");
        assert!((screen_y - 500.0).abs() < 1.0, "cluster centre y at {screen_y}, want 500");
    }

    /// A dense hub cluster (850 nodes, tightly packed near the origin) plus
    /// a one-sided scatter of far outliers (150 nodes, ~15% of the graph —
    /// the gravity-equilibrium ring `robust_bounds`' doc describes) sitting
    /// on the +x side at a moderate distance. Large enough a fraction to
    /// survive the 10th/90th-percentile trim and drag the BOX centre off
    /// the cluster, but nowhere near the ~50% needed to drag the MEDIAN off
    /// it — the real hub-and-ring shape (dense hub + a mostly-disconnected
    /// module ring settling one-sided) that motivated switching the PAN
    /// target from the box centre to the density centre.
    fn hub_and_ring_positions() -> Vec<(f32, f32)> {
        let cluster = (0..850).map(|i| {
            let x = (i % 34) as f32 - 17.0; // 34 distinct values, -17..16
            let y = (i / 34) as f32 - 12.5; // 25 rows, -12.5..11.5
            (x, y)
        });
        let ring = (0..150).map(|i| {
            let x = 60.0 + (i % 30) as f32; // one-sided: always well clear of the cluster
            let y = (i / 30) as f32 - 2.5; // narrow band, kept small vs. the cluster's y spread
            (x, y)
        });
        cluster.chain(ring).collect()
    }

    /// THE required behaviour change: on the hub-and-ring shape above, the
    /// 10th/90th-percentile BOX centre (`centroid`) is dragged toward the
    /// one-sided ring, but the density-weighted centre (`density_centre`,
    /// median x/y) is not — it stays on the hub, which is the visual mass
    /// the user is actually looking at. Expresses the INTENT (the dense
    /// mass is centred), not just a magic screen coordinate.
    #[test]
    fn fit_camera_density_centring_beats_box_centring_on_hub_and_ring() {
        let positions = hub_and_ring_positions();
        // Uniform weights: this test is about the median beating the box
        // on NODE COUNT alone, the property that motivated `density_centre`
        // in the first place — unrelated to the degree-weighting this
        // change adds on top. With equal weights `weighted_centre` and
        // `density_centre` coincide exactly (see the invariant test), so
        // this keeps asserting the same fact it always did.
        let weights = vec![1.0; positions.len()];
        let (w, h) = (1600.0_f32, 1000.0_f32);
        let (zoom, pan_x, pan_y) = fit_camera(&positions, &weights, w, h);

        let (mx, my) = density_centre(&positions);
        let screen_mx = mx * zoom + pan_x;
        let screen_my = my * zoom + pan_y;
        assert!(
            (screen_mx - w / 2.0).abs() < 2.0,
            "density centre x at {screen_mx}, want ~{}",
            w / 2.0
        );
        assert!(
            (screen_my - h / 2.0).abs() < 2.0,
            "density centre y at {screen_my}, want ~{}",
            h / 2.0
        );

        // The box centre must still be measurably off-centre on THIS
        // fixture — proving the two statistics genuinely diverge here, not
        // that the fixture happens to make them coincide.
        let (bx, _by) = centroid(&positions);
        let screen_bx = bx * zoom + pan_x;
        let box_err = (screen_bx - w / 2.0).abs();
        assert!(
            box_err > 50.0,
            "box centre should be measurably dragged off-centre by the ring, got err {box_err}"
        );
    }

    /// The zoom bound (10th/90th-percentile box) must be untouched by the
    /// pan-target change: every ring node — the outliers the box's extent
    /// was sized to admit — must still land inside the viewport once
    /// panned to the density centre, proving the centring fix did not
    /// shrink the frame to compensate.
    #[test]
    fn fit_camera_outliers_still_land_in_viewport_after_density_centring() {
        let positions = hub_and_ring_positions();
        let weights = vec![1.0; positions.len()];
        let (w, h) = (1600.0_f32, 1000.0_f32);
        let (zoom, pan_x, pan_y) = fit_camera(&positions, &weights, w, h);

        for &(x, y) in positions.iter().skip(850) {
            // ring nodes only
            let sx = x * zoom + pan_x;
            let sy = y * zoom + pan_y;
            assert!(
                (0.0..=w).contains(&sx),
                "ring node x={x} maps to sx={sx}, off-viewport (zoom={zoom}, pan_x={pan_x})"
            );
            assert!(
                (0.0..=h).contains(&sy),
                "ring node y={y} maps to sy={sy}, off-viewport (zoom={zoom}, pan_y={pan_y})"
            );
        }
    }

    /// Requirement 2 under actual WEIGHT variation (not just uniform
    /// weights): the ring is now the high-degree minority (see
    /// `weighted_centre_beats_density_centre_on_a_weighted_hub_and_ring`
    /// below) and the pan target shifts toward it — proving that shift
    /// doesn't push the *zoom bound* (still `robust_bounds`, untouched by
    /// weighting) off-frame for either side. Every node, cluster and ring
    /// alike, must still land on screen.
    #[test]
    fn fit_camera_weighted_outliers_still_land_in_viewport() {
        let positions = hub_and_ring_positions();
        let weights: Vec<f32> = (0..positions.len())
            .map(|i| if i < 850 { radius_for(0) } else { radius_for(5000) })
            .collect();
        let (w, h) = (1600.0_f32, 1000.0_f32);
        let (zoom, pan_x, pan_y) = fit_camera(&positions, &weights, w, h);

        for &(x, y) in &positions {
            let sx = x * zoom + pan_x;
            let sy = y * zoom + pan_y;
            assert!(
                (0.0..=w).contains(&sx),
                "node x={x} maps to sx={sx}, off-viewport (zoom={zoom}, pan_x={pan_x})"
            );
            assert!(
                (0.0..=h).contains(&sy),
                "node y={y} maps to sy={sy}, off-viewport (zoom={zoom}, pan_y={pan_y})"
            );
        }
    }

    /// Requirement 1: weighted centring beats unweighted on a hub-and-ring
    /// fixture at >=1000 nodes, where the "ring" side is dominated in
    /// WEIGHT (a 150-node minority of high-degree nodes) rather than in
    /// count — the inverse of `hub_and_ring_positions`' original framing,
    /// where the 850-node cluster was the visual mass because degree was
    /// assumed uniform. Here the 850-node cluster is low-degree (radius
    /// floor, 3.0) and the 150-node ring is high-degree (radius ceiling,
    /// 22.0) — same shape the maintainer measured as centre-of-rendered-ink
    /// drifting off the unweighted median in a real browser (a hub draws
    /// far more ink than its node count alone suggests). The plain rank
    /// median stays on the 850-node majority; the weighted median must
    /// follow the mass and land inside the high-degree ring instead.
    #[test]
    fn weighted_centre_beats_density_centre_on_a_weighted_hub_and_ring() {
        let positions = hub_and_ring_positions();
        assert!(positions.len() >= 1000, "call-graph fixture must be >=1000 nodes");
        let weights: Vec<f32> = (0..positions.len())
            .map(|i| if i < 850 { radius_for(0) } else { radius_for(5000) })
            .collect();

        let (wx, _wy) = weighted_centre(&positions, &weights);
        let (mx, _my) = density_centre(&positions);

        // The high-degree ring spans x in [60, 89]; the low-degree cluster
        // spans x in [-17, 16] — the two never overlap, so "which side" is
        // unambiguous.
        assert!(
            wx >= 60.0,
            "weighted centre x={wx} should land inside the high-degree ring (x>=60), the visual mass"
        );
        assert!(
            mx < 60.0,
            "sanity check: unweighted median x={mx} should stay in the low-degree \
             majority-by-count cluster (x<60) — otherwise this fixture doesn't \
             actually exercise the count-vs-weight divergence"
        );

        // Materially closer, not just nominally on the other side: measure
        // both against the ring's true centre (x=74.5) and demand a large
        // margin, not a hair's-width win.
        let ring_centre_x = 74.5;
        let weighted_dist = (wx - ring_centre_x).abs();
        let unweighted_dist = (mx - ring_centre_x).abs();
        assert!(
            weighted_dist + 30.0 < unweighted_dist,
            "weighted centre x={wx} (dist {weighted_dist} from ring centre) should be \
             materially closer to the high-degree ring than the unweighted median \
             x={mx} (dist {unweighted_dist})"
        );
    }

    #[test]
    fn density_centre_handles_empty_and_degenerate_input() {
        let (mx, my) = density_centre(&[]);
        assert!(mx.is_finite() && my.is_finite(), "empty input must not divide by zero or NaN");
        assert_eq!((mx, my), (0.0, 0.0), "empty input collapses to the origin, same as the box stats");

        let single = [(42.0, -7.0)];
        let (mx2, my2) = density_centre(&single);
        assert_eq!((mx2, my2), (42.0, -7.0), "a single node's median is itself");
    }

    #[test]
    fn recompute_cull_clears_a_now_invisible_selection() {
        let g = build_graph(&fixture(), Tier::Functions);
        let sim = simulate(4, &g.layout_edges, 42, &ForceConfig { max_ticks: 5, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);
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
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);

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

    #[test]
    fn should_autoplay_arms_only_the_function_tier() {
        // Maintainer spec, shipped-but-unverified until now: entering the
        // function tier auto-plays the roots animation; modules stay static
        // (small and legible without a wavefront). `code_graph_view.rs`
        // calls this exact predicate at the layout-settle site instead of
        // inlining the tier comparison, so the trigger condition is
        // unit-testable without a browser.
        assert!(should_autoplay(Tier::Functions));
        assert!(!should_autoplay(Tier::Modules));
    }

    #[test]
    fn advance_hop_reaches_a_hop_boundary_in_the_expected_frame_count() {
        // Readable-trace pacing check: at a healthy 60fps (~16.67ms/frame)
        // and HOP_DURATION_MS = 1500ms, a hop should take roughly
        // 1500 / 16.667 = 90 frames to cross — the concrete number that
        // makes "roughly 1-2s per hop" true in practice, not just in the
        // constant's value. A ±1 frame tolerance absorbs f32 accumulation
        // rounding, not a real pacing defect.
        let frame_ms = 1000.0 / 60.0;
        let (mut depth, mut progress) = (0i32, 0.0f32);
        let mut frames = 0usize;
        loop {
            let (d, p, _finished) = advance_hop(depth, progress, 5, frame_ms, HOP_DURATION_MS);
            frames += 1;
            if d != depth {
                break;
            }
            depth = d;
            progress = p;
        }
        let expected = (HOP_DURATION_MS / frame_ms).ceil() as i64;
        assert!(
            (frames as i64 - expected).abs() <= 1,
            "a hop should cross in ~{expected} frames at 60fps/{HOP_DURATION_MS}ms-per-hop, got {frames}"
        );
        assert!((45..=100).contains(&frames), "readable trace ~1.5s at 60fps is ~90 frames, got {frames}");
    }

    #[test]
    fn advance_hop_progress_is_monotonic_within_a_hop_and_crosses_every_boundary() {
        // Within a hop, `hop_progress` only ever climbs — never resets or
        // regresses frame-over-frame, so the edge growing this hop never
        // visibly snaps backward. Rollovers carry the overshoot remainder
        // (not a hard-reset 0) so the pacing does not drift from wall-clock
        // time across many hops — the CLEAN 0 guarantee is `set_anim_depth`'s
        // (explicit step/scrub) job, exercised separately below.
        let frame_ms = 1000.0 / 60.0;
        let (mut depth, mut progress) = (0i32, 0.0f32);
        let mut last_progress = -1.0f32;
        let mut crossings = 0;
        for _ in 0..400 {
            let (d, p, finished) = advance_hop(depth, progress, 3, frame_ms, HOP_DURATION_MS);
            assert!((0.0..1.0).contains(&p) || finished, "hop_progress stays a valid fraction");
            if d == depth {
                assert!(p > last_progress, "progress must strictly increase frame over frame within a hop");
            } else {
                crossings += 1;
            }
            depth = d;
            last_progress = p; // a fresh hop's own monotonic run starts from this frame's value
            progress = p;
            if finished {
                break;
            }
        }
        assert_eq!(crossings, 3, "every hop up to max_depth 3 crosses exactly once");
        assert_eq!(depth, 3);
    }

    #[test]
    fn stepping_and_scrubbing_land_on_clean_depth_boundaries_and_reset_progress() {
        let g = build_graph(&fixture(), Tier::Functions);
        let sim = simulate(4, &g.layout_edges, 42, &ForceConfig { max_ticks: 5, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);
        vs.anim_mode = AnimMode::Roots;
        vs.recompute_bfs();

        // Simulate mid-hop playback progress, then step — the step/scrub
        // path must land on an exact depth with hop_progress reset to 0,
        // never carrying the partial hop forward (the "clean depth
        // boundaries" requirement for step/scrub).
        vs.anim_hop_progress = 0.73;
        vs.set_anim_depth(1);
        assert_eq!(vs.anim_current_depth, 1);
        assert_eq!(vs.anim_hop_progress, 0.0, "a step/scrub always lands on a clean boundary");
        let expected =
            cull(&vs.graph, &vs.filter, Some(Wavefront { depth: &vs.anim_depth, current: 1, growth: true }));
        assert_eq!(vs.cull.nodes, expected.nodes, "the re-cull matches a plain depth-1 boundary");
        assert_eq!(vs.cull.edges, expected.edges, "no in-flight edge lingers from the pre-step progress");

        // Out-of-range depths clamp rather than panicking.
        vs.set_anim_depth(999);
        assert_eq!(vs.anim_current_depth, vs.anim_max_depth);
        vs.set_anim_depth(-5);
        assert_eq!(vs.anim_current_depth, 0);
    }

    #[test]
    fn advance_animation_reports_boundary_crossings_and_recomputes_cull_only_then() {
        let g = build_graph(&fixture(), Tier::Functions);
        let sim = simulate(4, &g.layout_edges, 42, &ForceConfig { max_ticks: 5, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);
        vs.anim_mode = AnimMode::Roots;
        vs.recompute_bfs(); // max_depth 2: main -> handler -> helper

        let (crossed, finished) = vs.advance_animation(HOP_DURATION_MS / 4.0);
        assert!(!crossed, "a quarter of a hop must not cross a depth boundary");
        assert!(!finished);
        assert!(vs.anim_hop_progress > 0.0 && vs.anim_hop_progress < 1.0);

        let (crossed, _finished) = vs.advance_animation(HOP_DURATION_MS);
        assert!(crossed, "enough elapsed time must cross into the next depth");
        assert_eq!(vs.anim_current_depth, 1);

        loop {
            let (_crossed, finished) = vs.advance_animation(HOP_DURATION_MS);
            if finished {
                break;
            }
        }
        assert_eq!(vs.anim_current_depth, vs.anim_max_depth, "playback stops exactly at the end");
        assert_eq!(vs.anim_hop_progress, 0.0);
    }

    // --- ≥1000-node interconnected-graph tests ------------------------------
    //
    // WHY ≥1000 (maintainer requirement): every render defect this feature
    // shipped was invisible on small fixtures — cull-set drift, interleave
    // shape errors, and wavefront double-visits only bite at real scale.
    // The generator is the shared deterministic one from
    // `code_graph_graph::tests_support` (same as the Task 1/2 scale tests).

    /// A 1000-function document over the shared `interconnected(1000, 3)`
    /// edge set. Classes are assigned deterministically (every 10th node
    /// dead, every 10th test-only, the rest prod) so the default filter's
    /// cull accounting is exactly checkable, and every 5th call is dynamic
    /// so both edge kinds flow through.
    fn fixture_1000() -> (CodeGraph, usize) {
        use crate::code_graph_graph::tests_support;
        let (n, edges) = tests_support::interconnected(1000, 3);
        let functions: Vec<serde_json::Value> = (0..n)
            .map(|i| {
                let dead = i % 10 == 1;
                let test = i % 10 == 3;
                serde_json::json!({
                    "id": format!("f{i}"), "symbol": format!("pkg.fn{i}"), "pkg": "pkg",
                    "file": "f.go", "line": i + 1, "kind": "func", "exported": false,
                    "test": test, "root": i == 0, "generated": false,
                    "reachable": !dead, "prod_reachable": !dead && !test,
                    "signature": {"params": [], "results": []},
                    "fan_in": 0, "fan_out": 0
                })
            })
            .collect();
        let calls: Vec<serde_json::Value> = edges
            .iter()
            .enumerate()
            .map(|(k, &(a, b))| {
                serde_json::json!({
                    "from": format!("f{a}"), "to": format!("f{b}"),
                    "site_file": "f.go", "site_line": 1,
                    "kind": if k % 5 == 0 { "dynamic" } else { "static" }
                })
            })
            .collect();
        let doc: CodeGraph = serde_json::from_value(serde_json::json!({
            "contract_version": "magma-code-graph/1", "generator": "magma",
            "language": "go", "module": "example.com/x", "sha": "a",
            "tree": "clean", "fidelity": "rta", "computable": true,
            "functions": functions, "calls": calls
        }))
        .expect("1000-node fixture parses");
        (doc, n)
    }

    /// FilterState with every class and edge kind shown — the full-scale
    /// upload case (all 1000 nodes reach the GPU).
    fn show_everything() -> FilterState {
        FilterState {
            show_prod_reachable: true,
            show_dead: true,
            show_test_only: true,
            show_generated: true,
            show_static: true,
            show_dynamic: true,
        }
    }

    #[test]
    fn cull_at_1000_interconnected_nodes_is_consistent_and_accounts_counts() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        assert_eq!(g.node_count(), n);
        assert!(g.directed_edges.len() > n, "the fixture is interconnected");

        let c = cull(&g, &FilterState::default(), None);
        // "showing N of M" accounting: the default filter shows prod-reachable
        // only, and the visible map, the node list, and the flag slices must
        // all agree on N.
        let expected_nodes = g.prod_reachable.iter().filter(|&&p| p).count();
        assert_eq!(c.nodes.len(), expected_nodes, "default filter shows prod-reachable only");
        assert!(c.nodes.len() < n, "the default filter must actually cull at scale");
        assert_eq!(
            c.visible.iter().filter(|&&v| v).count(),
            c.nodes.len(),
            "visible map and surviving node list agree"
        );
        assert!(c.nodes.iter().all(|&i| c.visible[i] && g.prod_reachable[i]));

        // Every surviving edge has both endpoints surviving, and every edge
        // whose endpoints survive IS in the set (no drift in either direction).
        for &(a, b, _) in &c.edges {
            assert!(c.visible[a] && c.visible[b], "edge endpoint was culled but the edge survived");
        }
        let expected_edges = g
            .directed_edges
            .iter()
            .filter(|&&(a, b, _)| c.visible[a] && c.visible[b])
            .count();
        assert_eq!(c.edges.len(), expected_edges, "cull drops exactly the endpoint-culled edges");
    }

    #[test]
    fn dynamic_state_interleaves_are_exact_4f32_per_instance_at_1000_nodes() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        let c = cull(&g, &show_everything(), None);
        assert_eq!(c.nodes.len(), n, "show-everything uploads the full graph");
        assert!(c.edges.len() > n, "full-scale edge upload");

        // Nodes: [alpha, glow, colorMix, hopAge] per uploaded instance.
        let ns = node_state(&g, &c, None, AnimMode::Off, &[], 0, 0.0, false);
        assert_eq!(ns.len(), c.nodes.len() * 4, "exactly 4 f32 per uploaded node");
        for slot in ns.chunks_exact(4) {
            assert!(
                slot[0].is_finite() && slot[1].is_finite() && slot[2].is_finite(),
                "node state must stay finite at scale"
            );
            assert_eq!(slot[3], 0.0, "node hopAge slot stays zero outside an animation");
        }

        // Edges: [alpha, colorMix, progress, hopAge] per uploaded instance —
        // the Off path always draws full length (progress 1.0, formerly
        // always-zero padding — see `gl/shaders.rs`'s `EDGE_VS`).
        let es = edge_state(&c, None, AnimMode::Off, &[], 0, 0.0, false);
        assert_eq!(es.len(), c.edges.len() * 4, "exactly 4 f32 per uploaded edge");
        for slot in es.chunks_exact(4) {
            assert!(slot[0].is_finite() && slot[1].is_finite(), "edge state must stay finite");
            assert_eq!(slot[2], 1.0, "edge fully drawn outside an animation");
            assert_eq!(slot[3], 0.0, "edge hopAge slot stays zero outside an animation");
        }
    }

    #[test]
    fn wavefront_depth_field_at_1000_nodes_assigns_each_node_once() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        assert_eq!(g.roots, vec![0], "the fixture has exactly one root");

        let layers = g.index.bfs(Direction::Outbound, &g.roots);
        let depth = depth_field(n, &layers);
        assert_eq!(depth.len(), n, "the depth field spans the full graph");

        // Every node gets a depth or the -1 sentinel, and no node is visited
        // twice (a duplicate would silently overwrite and desync the counts).
        let layered: usize = layers.iter().map(|l| l.len()).sum();
        let reached = depth.iter().filter(|&&d| d >= 0).count();
        assert_eq!(layered, reached, "no node appears in two BFS layers");
        assert_eq!(
            reached + depth.iter().filter(|&&d| d == -1).count(),
            n,
            "every node is assigned a depth or the sentinel"
        );
        assert!(reached > 900, "the interconnected fixture is mostly reachable, got {reached}");
        for (d, layer) in layers.iter().enumerate() {
            for &i in layer {
                assert_eq!(depth[i], d as i32, "depth matches the layer index");
            }
        }

        // The wavefront composes with the filter cull at scale — REQUIREMENT:
        // un-reached nodes/edges are culled from the upload while animating,
        // not merely faded, so a real wavefront at depth 1 (of the fixture's
        // much deeper BFS) must upload strictly fewer nodes/edges than the
        // filter-only cull, and every value must stay finite.
        let base = cull(&g, &FilterState::default(), None);
        let wave = Wavefront { depth: &depth, current: 1, growth: true };
        let c = cull(&g, &FilterState::default(), Some(wave));
        assert!(c.nodes.len() < base.nodes.len(), "the wavefront must actually cull at scale");
        assert!(c.nodes.iter().all(|&i| depth[i] >= 0 && depth[i] <= 1), "only depth 0/1 nodes upload");

        let ns = node_state(&g, &c, None, AnimMode::Roots, &depth, 1, 0.6, false);
        assert_eq!(ns.len(), c.nodes.len() * 4);
        assert!(ns.iter().all(|v| v.is_finite()), "animated node state stays finite at scale");
        let es = edge_state(&c, None, AnimMode::Roots, &depth, 1, 0.6, false);
        assert_eq!(es.len(), c.edges.len() * 4);
        assert!(es.iter().all(|v| v.is_finite()), "animated edge state stays finite at scale");
        assert!(!c.edges.is_empty(), "sanity: the fixture's depth-1 wavefront has in-flight/settled edges");
        for &(lo, hi, _) in &c.edges {
            assert!(depth[lo] <= depth[hi], "cull reorders every edge lower-depth-endpoint-first");
        }
    }

    // --- Comet-trail brightness decay (1.8.0 release item) -----------------
    //
    // "as we're drawing the initial hairball during load up, can we keep the
    // new elements bright and gradually fade out the older elements so that
    // users can better see what's happening?" — the actual brightness curve
    // runs on the GPU every frame (`gl/shaders.rs`'s `NODE_VS`/`EDGE_VS`, not
    // unit-testable without a browser), so these tests pin the PROPERTIES of
    // its Rust-side twin (`decay_brightness`) and the age input it consumes
    // (`hop_age`, exercised indirectly through `node_state`/`edge_state`).

    #[test]
    fn decay_brightness_is_brightest_at_or_before_activation_and_never_negative() {
        // age <= 0 covers both "not yet activated" and (edges only) "still
        // mid-growth" — hop_age's doc explains why both collapse to the same
        // freshest treatment rather than a separate branch.
        for age in [-10.0_f32, -1.0, -0.001, 0.0] {
            assert_eq!(
                decay_brightness(age, NODE_DECAY_HOPS, TRAIL_ALPHA),
                1.0,
                "age {age} <= 0 must be full brightness"
            );
        }
    }

    #[test]
    fn decay_brightness_is_monotonically_decreasing_and_floors_at_the_span() {
        let floor = TRAIL_ALPHA;
        let span = NODE_DECAY_HOPS;
        let samples: Vec<f32> = (0..=40).map(|i| i as f32 * span / 40.0).collect();
        let values: Vec<f32> = samples.iter().map(|&age| decay_brightness(age, span, floor)).collect();
        for w in values.windows(2) {
            assert!(
                w[1] <= w[0] + f32::EPSILON,
                "brightness must never increase as age grows: {} then {}",
                w[0],
                w[1]
            );
        }
        assert!((values[0] - 1.0).abs() < 1e-6, "age 0 is peak brightness");
        assert!((*values.last().unwrap() - floor).abs() < 1e-6, "age == span lands exactly on the floor");

        // Beyond the span the curve holds flat — never dips below the floor,
        // which is the whole point (already-revealed elements must stay
        // VISIBLE, not fade to nothing).
        for age in [span, span * 2.0, span * 100.0, f32::MAX] {
            let b = decay_brightness(age, span, floor);
            assert!(b >= floor - 1e-6, "brightness {b} at age {age} fell below the floor {floor}");
            assert!((b - floor).abs() < 1e-6, "brightness past the span must hold exactly at the floor");
        }
    }

    #[test]
    fn decay_brightness_floor_is_clearly_above_the_culled_absent_state() {
        // "Must stay clearly above the culled/absent state" — a culled
        // element is never uploaded at all (effectively alpha 0). Both
        // floors reused here (TRAIL_ALPHA for nodes, EDGE_ALPHA_FLOOR for
        // edges) predate this decay work — they are the exact resting
        // values the pre-decay code already shipped and proved legible.
        assert!(TRAIL_ALPHA > 0.2, "node floor must be comfortably above zero");
        assert!(EDGE_ALPHA_FLOOR > 0.15, "edge floor must be comfortably above zero");
    }

    #[test]
    fn hop_age_is_zero_at_activation_negative_mid_growth_and_grows_with_the_wavefront() {
        // A node at depth 2, wavefront currently at depth 2 with no hop
        // progress yet: it just became fully reached this instant.
        assert_eq!(hop_age(2, 2, 0.0), 0.0, "just-activated element has age 0");
        // Half a hop later (still depth 2 current): age has grown by 0.5.
        assert_eq!(hop_age(2, 2, 0.5), 0.5);
        // An in-flight edge's higher-depth endpoint is one hop AHEAD of
        // current — hop_progress in [0,1) keeps this negative until the
        // boundary crossing, matching "still growing = brightest".
        assert!(hop_age(3, 2, 0.9) < 0.0, "still-growing element must read as not-yet-activated");
        // Three whole hops after activation (current has advanced 3 hops
        // past this element's own depth): age == 3.0 exactly.
        assert_eq!(hop_age(0, 3, 0.0), 3.0);
    }

    #[test]
    fn node_and_edge_hop_age_reflects_recency_at_1000_nodes_and_reduced_motion_bypasses_decay() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        let layers = g.index.bfs(Direction::Outbound, &g.roots);
        let depth = depth_field(n, &layers);
        // A wavefront several hops in, mid-hop, so both "just arrived" and
        // "long settled" nodes are present in the same upload.
        let current = 3;
        let hop_progress = 0.4;
        let wave = Wavefront { depth: &depth, current, growth: true };
        let c = cull(&g, &FilterState::default(), Some(wave));
        assert!(c.nodes.len() > 10, "sanity: the wavefront has reached a meaningful slice of 1000 nodes");

        // Motion enabled: every uploaded node's age is non-negative (a node
        // is only uploaded once FULLY reached — see `cull`'s doc — so it can
        // never be "mid growth" the way an in-flight edge can) and strictly
        // larger for nodes revealed longer ago, i.e. lower BFS depth.
        let ns = node_state(&g, &c, None, AnimMode::Roots, &depth, current, hop_progress, false);
        for (slot, &gi) in ns.chunks_exact(4).zip(&c.nodes) {
            let age = slot[3];
            assert!(age >= 0.0, "a fully-reached node must never have a negative age, got {age}");
            let expected = hop_age(depth[gi], current, hop_progress);
            assert!((age - expected).abs() < 1e-5, "uploaded age must match hop_age exactly");
        }
        // The freshest cohort (depth == current) is younger than an older
        // one (depth == current - 1), proving the ordering "freshly
        // activated is brightest, older is dimmer" holds at scale.
        let freshest_idx = c.nodes.iter().position(|&i| depth[i] == current);
        let older_idx = c.nodes.iter().position(|&i| depth[i] == current - 1);
        if let (Some(fi), Some(oi)) = (freshest_idx, older_idx) {
            let age_fresh = ns[fi * 4 + 3];
            let age_older = ns[oi * 4 + 3];
            assert!(age_fresh < age_older, "a node revealed this hop must be younger than one revealed last hop");
        }

        // prefers-reduced-motion: reduce — every age collapses to the
        // decay-identity value (0.0), regardless of real recency. This is
        // the "no new motion for those users" contract: `decay_brightness`
        // treats 0.0 the same as "just activated", so nothing decays.
        let ns_reduced = node_state(&g, &c, None, AnimMode::Roots, &depth, current, hop_progress, true);
        assert!(
            ns_reduced.chunks_exact(4).all(|slot| slot[3] == 0.0),
            "reduced motion must bypass decay entirely — every node age must be exactly 0.0"
        );
        let es_reduced = edge_state(&c, None, AnimMode::Roots, &depth, current, hop_progress, true);
        assert!(
            es_reduced.chunks_exact(4).all(|slot| slot[3] == 0.0),
            "reduced motion must bypass decay entirely — every edge age must be exactly 0.0"
        );
    }

    /// Regression for the P0 report's second symptom ("changing options...
    /// clears the graph and never rerenders"): a filter checkbox or
    /// animation-mode change must ONLY affect what is culled/highlighted,
    /// never the force-simulated layout itself. `ViewState::recompute_cull`
    /// and `recompute_bfs` are exactly what the view's filter effect and
    /// `set_anim_mode` call on every option change (`code_graph_view.rs`);
    /// this proves neither one perturbs `positions` — the same positions
    /// the view's progressive layout driver is (or isn't) still ticking —
    /// at real (1000+ interconnected node) scale, not just on the 4-node
    /// fixture above.
    #[test]
    fn filter_and_anim_mode_changes_never_touch_the_settled_positions_at_1000_nodes() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        let sim = simulate(n, &g.layout_edges, 42, &ForceConfig { max_ticks: 20, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let before = positions.clone();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);

        // Every filter checkbox the toolbar exposes, flipped one at a time.
        for flip in [
            |f: &mut FilterState| f.show_prod_reachable = !f.show_prod_reachable,
            |f: &mut FilterState| f.show_dead = !f.show_dead,
            |f: &mut FilterState| f.show_test_only = !f.show_test_only,
            |f: &mut FilterState| f.show_generated = !f.show_generated,
            |f: &mut FilterState| f.show_static = !f.show_static,
            |f: &mut FilterState| f.show_dynamic = !f.show_dynamic,
        ] {
            flip(&mut vs.filter);
            vs.recompute_cull();
            assert_eq!(vs.positions, before, "a filter change must never move a single node");
        }

        // Every animation mode the toolbar exposes.
        vs.selected = Some(0);
        for mode in [AnimMode::Roots, AnimMode::Outbound, AnimMode::Inbound, AnimMode::Off] {
            vs.anim_mode = mode;
            vs.recompute_bfs();
            assert_eq!(vs.positions, before, "an animation-mode change must never move a single node");
        }

        // The cull/BFS churn above must have actually done something (i.e.
        // this test is not vacuously true because nothing changed at all).
        assert_ne!(vs.cull.nodes.len(), 0, "sanity: the graph still has visible nodes");
    }

    // --- Defect 1 regression: hit-testing vs. selection lifetime ------------
    //
    // The maintainer-reported bug: entering the function tier auto-plays the
    // roots wavefront, which culls the render down to a handful of nodes
    // ("Showing 3 of 17814"); hit-testing rejected everything the wavefront
    // hadn't drawn yet, so clicks landed on nothing, and any selection that
    // did land was silently dropped as the wavefront advanced past it. The
    // fix splits `Cull::visible` (render mask, filter AND wavefront) from
    // `Cull::filter_visible` (hit-test / selection-lifetime mask, filter
    // ONLY) — these four tests pin the distinction at the mandated ≥1000-node
    // scale.

    #[test]
    fn hit_test_mask_admits_animation_culled_nodes_but_rejects_filter_culled_ones_at_1000_nodes() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        let layers = g.index.bfs(Direction::Outbound, &g.roots);
        let depth = depth_field(n, &layers);
        let wave = Wavefront { depth: &depth, current: 0, growth: true };
        let c = cull(&g, &FilterState::default(), Some(wave));

        // An animation-culled node: prod-reachable (so the default filter
        // shows it) but not yet reached by the depth-0 wavefront. Hit-testing
        // (`code_graph_view.rs`'s on_click, which filters by
        // `cull.filter_visible`) must still treat it as a legitimate target
        // even though it is not currently drawn.
        let anim_culled = (0..n)
            .find(|&i| g.prod_reachable[i] && depth[i] > 0)
            .expect("the interconnected fixture has a node beyond depth 0");
        assert!(c.filter_visible[anim_culled], "animation-culled node must remain hit-testable");
        assert!(!c.visible[anim_culled], "sanity: it really is culled from the render");
        assert!(!c.nodes.contains(&anim_culled), "sanity: not in the uploaded set either");

        // A filter-culled node: dead or test-only, excluded by the default
        // (prod-reachable-only) filter regardless of the wavefront. Hit-testing
        // must reject it exactly as before this fix.
        let filter_culled =
            (0..n).find(|&i| !g.prod_reachable[i]).expect("fixture has dead/test nodes");
        assert!(!c.filter_visible[filter_culled], "filter-culled node must never be hit-testable");
    }

    #[test]
    fn selection_survives_animation_advance_at_1000_nodes() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        let sim = simulate(n, &g.layout_edges, 42, &ForceConfig { max_ticks: 20, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);

        vs.anim_mode = AnimMode::Roots;
        vs.recompute_bfs();
        // Pick a node several hops out — reached well after depth 0, so it
        // starts this wavefront animation-culled (present in `filter_visible`,
        // absent from `cull.nodes`/`visible`).
        let target = (0..vs.graph.node_count())
            .find(|&i| vs.graph.prod_reachable[i] && vs.anim_depth[i] > 1)
            .expect("the interconnected fixture has a node beyond depth 1");
        vs.selected = Some(target);
        assert!(!vs.cull.nodes.contains(&target), "sanity: the target starts animation-culled");

        // Advance the wavefront hop by hop to the end. At every boundary
        // crossing `recompute_cull` runs — the selection must never be
        // dropped, whether the target is still ahead of the wavefront or has
        // since been passed.
        loop {
            let (_crossed, finished) = vs.advance_animation(HOP_DURATION_MS);
            assert_eq!(vs.selected, Some(target), "animation progress must never drop the selection");
            if finished {
                break;
            }
        }
        assert!(vs.cull.nodes.contains(&target), "by the end the wavefront has drawn the target too");
    }

    #[test]
    fn selection_is_still_cleared_when_a_filter_hides_it_at_1000_nodes() {
        let (doc, n) = fixture_1000();
        let g = build_graph(&doc, Tier::Functions);
        let sim = simulate(n, &g.layout_edges, 42, &ForceConfig { max_ticks: 20, ..ForceConfig::default() });
        let positions: Vec<(f32, f32)> = sim.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        let mut vs = ViewState::new(g, positions, sim.tree, 0.0, 0.0, 1.0, false);

        // Show everything, select a dead node, then flip back to the default
        // (prod-reachable-only) filter — a REAL hide, not merely a wavefront
        // that hasn't reached it yet — which must still clear the selection.
        vs.filter = show_everything();
        vs.recompute_cull();
        let dead_node = (0..n).find(|&i| !vs.graph.prod_reachable[i]).expect("fixture has dead nodes");
        vs.selected = Some(dead_node);
        vs.filter = FilterState::default();
        vs.recompute_cull();
        assert_eq!(vs.selected, None, "a filter-hidden selection must still be cleared");
    }

    /// A heavy-tailed degree distribution shaped like a real call graph:
    /// most functions call almost nothing, a handful are hubs. Proportions
    /// and the 888 maximum are taken from the magma corpus this viewer is
    /// built to render (17,814 Go functions, median degree 3).
    fn heavy_tailed_degrees(n: usize) -> Vec<u32> {
        (0..n)
            .map(|i| match i * 100 / n {
                0..=4 => 0,             // 5% isolated
                5..=76 => 1 + (i % 9) as u32, // 72% in the crowded 1-9 band
                77..=94 => 10 + (i % 28) as u32,
                95..=98 => 38 + (i % 62) as u32,
                _ => 100 + (i % 789) as u32, // the hub tail, up to 888
            })
            .collect()
    }

    #[test]
    fn radius_separates_the_common_range_instead_of_saturating_both_ends() {
        // WHY: node size is the reader's only at-a-glance cue for "how
        // connected is this?". A mapping can be perfectly monotonic and still
        // fail that job by spending its visual range where no nodes live. The
        // previous sqrt mapping did exactly that on real data — three
        // quarters of nodes within a 3px band, and the busiest 46 all pinned
        // at the ceiling — so this test pins the two SEPARATION properties
        // that motivated the change, not the formula itself. A future remap
        // is free to change the curve as long as it still separates.
        const N: usize = 1200; // >1000 interconnected-scale sample (maintainer rule)
        let degrees = heavy_tailed_degrees(N);
        let radii: Vec<f32> = degrees.iter().map(|&d| radius_for(d)).collect();

        // 1. The ceiling must stay scarce: hubs are the nodes a reader most
        //    needs to tell apart, so they must not collapse into one size.
        let at_ceiling = radii.iter().filter(|r| **r >= RADIUS_MAX - 0.01).count();
        assert!(
            at_ceiling * 100 / N < 2,
            "hubs saturating the ceiling stop being distinguishable: {at_ceiling}/{N}"
        );

        // 2. The crowded low-degree band must actually spread. Under sqrt,
        //    degree 1 and degree 9 differed by ~3.4px; the whole point of the
        //    log remap is to widen precisely this range.
        let spread = radius_for(9) - radius_for(1);
        assert!(spread > 4.0, "low-degree band too compressed to read: {spread:.1}px");

        // 3. Still monotonic and still bounded — a size that shrank with
        //    degree, or escaped the clamp, would be worse than compressed.
        assert_eq!(radius_for(0), RADIUS_MIN);
        assert_eq!(radius_for(u32::MAX), RADIUS_MAX);
        for w in degrees.windows(2) {
            if w[1] > w[0] {
                assert!(radius_for(w[1]) >= radius_for(w[0]), "radius must not shrink with degree");
            }
        }
    }

    #[test]
    fn radius_meaning_is_absolute_not_relative_to_the_document() {
        // WHY: the rejected alternative (rank/percentile) would have used far
        // more of the visual span, but it makes radius mean "busier than N%
        // of THIS document" — the same function would change size between
        // repositories, and a median node would render as a hub. Pin that a
        // given degree maps to a given radius regardless of what else is in
        // the graph, so nobody "improves" the spread by reintroducing that.
        // Pinned absolute values. A document-relative mapping cannot satisfy
        // these from `degree` alone, so this fails loudly if anyone swaps the
        // curve for a rank/percentile one — which is the actual risk, since
        // percentile scores far better on raw visual spread and is the
        // tempting "fix" for a reader who only sees the spread number.
        for (degree, expected) in [(0u32, 3.0f32), (1, 5.0), (3, 7.0), (10, 9.9), (100, 16.3)] {
            let got = radius_for(degree);
            assert!(
                (got - expected).abs() < 0.05,
                "degree {degree} must map to {expected}px regardless of the surrounding \
                 document, got {got:.2}"
            );
        }
        // The busiest node in the real corpus should reach the ceiling and
        // not sail far past it — that is what `RADIUS_PX_PER_DOUBLING` buys.
        assert_eq!(radius_for(888), RADIUS_MAX);
        assert!(radius_for(444) < RADIUS_MAX, "the ceiling should be reached at the tail, not before it");
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::*;

    /// Load Architext's own code graph, which is the artifact the viewer shows.
    fn real_graph() -> Option<CodeGraph> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/architext/data/code-graph.json");
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    #[test]
    fn measure_cluster_separation() {
        let Some(cg) = real_graph() else { return };
        let graph = build_graph(&cg, Tier::Functions);
        let (ids, count) = cluster_ids(&cg, Tier::Functions);
        for pull in [0.05_f64, 0.12, 0.25, 0.5, 1.0] {
            let cfg = ForceConfig { cluster_pull: pull, ..ForceConfig::default() };
            let anchors = cluster_anchors(&ids, count, &graph.layout_edges, &cfg);
            let r = crate::force_layout::simulate_clustered(
                graph.node_count(), &graph.layout_edges, LAYOUT_SEED, &cfg, &anchors);
            let p = &r.positions;
            // mean within-cluster radius vs overall extent
            let mut sum_c = vec![(0.0_f64, 0.0_f64); count];
            let mut n_c = vec![0usize; count];
            for (i, &(x, y)) in p.iter().enumerate() {
                sum_c[ids[i]].0 += x; sum_c[ids[i]].1 += y; n_c[ids[i]] += 1;
            }
            let cen: Vec<(f64,f64)> = sum_c.iter().zip(&n_c)
                .map(|(&(sx,sy), &n)| if n>0 {(sx/n as f64, sy/n as f64)} else {(0.0,0.0)}).collect();
            let within: f64 = p.iter().enumerate()
                .map(|(i,&(x,y))| {let c=cen[ids[i]]; ((x-c.0).powi(2)+(y-c.1).powi(2)).sqrt()})
                .sum::<f64>() / p.len() as f64;
            let ext = p.iter().map(|&(x,y)| x.abs().max(y.abs())).fold(0.0_f64, f64::max);
            let lobe_area: f64 = (0..count)
                .map(|c| {
                    let n = n_c[c] as f64;
                    std::f64::consts::PI * (LOBE_RADIUS_K * n.sqrt()).powi(2)
                })
                .sum();
            let field = (2.0 * ext).powi(2);
            println!(
                "PULL {pull}: within {within:.0}  extent {ext:.0}  ratio {:.3}  packing {:.3}",
                within / ext.max(1.0),
                lobe_area / field.max(1.0)
            );
        }
    }

    #[test]
    fn clustering_actually_partitions_the_real_graph() {
        // Guards the thing three rebuilds failed to change on screen: if
        // cluster_ids returns one cluster (or one per node) the anchors are
        // useless and the layout is the old sphere, with nothing in the code
        // to say so.
        let Some(cg) = real_graph() else {
            eprintln!("no code-graph.json checked in; skipping");
            return;
        };
        for tier in [Tier::Modules, Tier::Functions] {
            let (ids, count) = cluster_ids(&cg, tier);
            assert!(!ids.is_empty(), "{tier:?}: no cluster ids");
            assert!(count > 1, "{tier:?}: only {count} cluster — nothing to separate");
            assert!(
                count < ids.len() / 2,
                "{tier:?}: {count} clusters over {} nodes is barely a partition",
                ids.len()
            );
            let anchors = cluster_anchors(
                &ids,
                count,
                &build_graph(&cg, tier).layout_edges,
                &ForceConfig { cluster_pull: CLUSTER_PULL, ..ForceConfig::default() },
            );
            assert_eq!(anchors.len(), ids.len(), "{tier:?}: one anchor per node");
            let distinct: std::collections::BTreeSet<(i64, i64)> = anchors
                .iter()
                .map(|&(x, y)| ((x * 10.0) as i64, (y * 10.0) as i64))
                .collect();
            assert!(
                distinct.len() > 1,
                "{tier:?}: every anchor is the same point, so nothing is pulled apart"
            );
            println!("CLUSTERS {tier:?}: {count} clusters, {} nodes, {} distinct anchors",
                ids.len(), distinct.len());
        }
    }
}
