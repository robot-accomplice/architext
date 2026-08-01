//! SPIKE A: hand-written Barnes-Hut force-directed layout, pure Rust, no JS.
//!
//! Throwaway prototype for the "does a pure-Rust force layout scale to a real
//! Go call graph" question. NOT wired into the production data contract in
//! any load-bearing way — `code_graph_canvas.rs` is the only caller.
//!
//! Fruchterman-Reingold style attraction/repulsion, with a quadtree
//! (Barnes-Hut) approximation for repulsion so the per-tick cost is
//! O(n log n) rather than O(n^2). The RNG is a seeded splitmix64 so a given
//! graph always lays out the same way (deterministic screenshots).

/// One simulated body: position only. Rendering radius/label live alongside
/// in `code_graph_canvas.rs` (this module only knows physics). Velocity is
/// deliberately NOT part of the state — see `ForceConfig` for why.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub x: f64,
    pub y: f64,
}

/// Tunables for the classic Fruchterman-Reingold scheme: `k` is the ideal
/// edge length (repulsion ~ k^2/d, attraction ~ d^2/k), and a per-tick
/// "temperature" caps how far any body may move, cooling linearly to zero
/// over `max_ticks`. This is the textbook stability mechanism — an earlier
/// draft of this spike used velocity + damping with fixed force constants
/// and it collapsed the whole 205-node module tier into a single point at
/// certain scales (attraction growing unbounded with distance while
/// repulsion decayed as 1/d^2 let a few tightly-connected hubs drag
/// everything to the centroid). The temperature cap bounds worst-case
/// displacement regardless of force magnitude, which is what makes this
/// version stable at both 205 and 17,561 nodes with the SAME constants.
#[derive(Clone, Copy)]
pub struct ForceConfig {
    pub theta: f64,
    /// Ideal edge length. Deliberately independent of node count: the
    /// natural layout footprint grows ~sqrt(n) and the viewer fits-to-view
    /// afterward (Obsidian does the same — physics doesn't know about the
    /// viewport).
    pub k: f64,
    /// Fraction of a body's distance from the origin pulled back per tick —
    /// keeps a disconnected component from drifting to infinity under pure
    /// repulsion. Small relative to `k` so it never dominates real edges.
    pub gravity: f64,
    pub max_ticks: usize,
    /// Stop early once the max per-tick displacement drops below this.
    pub convergence_eps: f64,
}

impl Default for ForceConfig {
    fn default() -> Self {
        Self { theta: 0.85, k: 60.0, gravity: 0.02, max_ticks: 400, convergence_eps: 0.05 }
    }
}

/// Deterministic seeded PRNG (splitmix64) — no `rand` dependency for a
/// throwaway spike that only needs reproducible jitter.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    /// Uniform float in `[lo, hi)`.
    fn next_f64(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + u * (hi - lo)
    }
}

/// Axis-aligned bounding box, used both to size the quadtree root and (via
/// `Quad::contains`) to prune descent.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    cx: f64,
    cy: f64,
    half: f64,
}

impl Bounds {
    fn quadrant(&self, idx: usize) -> Bounds {
        let half = self.half / 2.0;
        let (dx, dy) = match idx {
            0 => (-half, -half),
            1 => (half, -half),
            2 => (-half, half),
            _ => (half, half),
        };
        Bounds { cx: self.cx + dx, cy: self.cy + dy, half }
    }
    fn quadrant_of(&self, x: f64, y: f64) -> usize {
        match (x >= self.cx, y >= self.cy) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }
}

/// A Barnes-Hut quadtree node. `Empty` and `Leaf` are the base cases;
/// `Internal` carries the aggregate center-of-mass used for the
/// far-field approximation.
enum QNode {
    Empty,
    Leaf { idx: usize, x: f64, y: f64 },
    Internal { mass: f64, cx: f64, cy: f64, children: Box<[QNode; 4]>, bounds: Bounds },
}

/// The quadtree over the CURRENT tick's positions. Rebuilt every tick (the
/// positions move), which is the standard Barnes-Hut trade-off: O(n log n)
/// build + O(n log n) force evaluation per tick beats O(n^2) once n is more
/// than a few hundred.
pub struct QuadTree {
    root: QNode,
}

impl QuadTree {
    fn build(points: &[(f64, f64)]) -> Self {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(x, y) in points {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        if !min_x.is_finite() {
            // Empty input — degenerate root, never inserted into.
            return Self { root: QNode::Empty };
        }
        let half = ((max_x - min_x).max(max_y - min_y) / 2.0).max(1.0) * 1.05;
        let bounds = Bounds { cx: (min_x + max_x) / 2.0, cy: (min_y + max_y) / 2.0, half };

        let mut root = QNode::Empty;
        for (idx, &(x, y)) in points.iter().enumerate() {
            insert(&mut root, bounds, idx, x, y, 0);
        }
        Self { root }
    }

    /// Barnes-Hut repulsion on body `idx` at `(x, y)`: walk the tree, treating
    /// any node whose `size/distance < theta` as a single point mass at its
    /// center of mass (the far-field approximation), otherwise recursing.
    fn repulsion_at(&self, idx: usize, x: f64, y: f64, cfg: &ForceConfig) -> (f64, f64) {
        let mut fx = 0.0;
        let mut fy = 0.0;
        accumulate_repulsion(&self.root, idx, x, y, cfg, &mut fx, &mut fy);
        (fx, fy)
    }

    /// Point-in-radius hit test — reuses the same tree built for the final
    /// tick's layout so a click doesn't need a fresh linear scan. Returns the
    /// nearest node within `max_r`, preferring the closest when several
    /// circles overlap the click point.
    pub fn query_point(&self, x: f64, y: f64, max_r: f64) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        query_node(&self.root, x, y, max_r, &mut best);
        best.map(|(idx, _)| idx)
    }

    /// Build a hit-test tree directly over externally-supplied f32 positions
    /// (Plan D Task 2: a layout-cache hit has settled positions but no live
    /// `Simulation` to ask for a tree). `build` stays private to this module;
    /// this is the one sanctioned entry point for rebuilding a tree from
    /// scratch outside it.
    pub fn from_positions_f32(positions: &[(f32, f32)]) -> Self {
        let points: Vec<(f64, f64)> = positions.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
        Self::build(&points)
    }
}

const MAX_DEPTH: u32 = 40;

fn insert(node: &mut QNode, bounds: Bounds, idx: usize, x: f64, y: f64, depth: u32) {
    match node {
        QNode::Empty => {
            *node = QNode::Leaf { idx, x, y };
        }
        QNode::Leaf { idx: eidx, x: ex, y: ey } => {
            let (eidx, ex, ey) = (*eidx, *ex, *ey);
            // Coincident (or depth-exhausted) points: keep the first, drop the
            // second's position influence into the same cell rather than
            // recursing forever on identical coordinates.
            if depth >= MAX_DEPTH {
                return;
            }
            let mut children: Box<[QNode; 4]> =
                Box::new([QNode::Empty, QNode::Empty, QNode::Empty, QNode::Empty]);
            let qe = bounds.quadrant_of(ex, ey);
            insert(&mut children[qe], bounds.quadrant(qe), eidx, ex, ey, depth + 1);
            let qn = bounds.quadrant_of(x, y);
            insert(&mut children[qn], bounds.quadrant(qn), idx, x, y, depth + 1);
            *node = QNode::Internal { mass: 2.0, cx: (ex + x) / 2.0, cy: (ey + y) / 2.0, children, bounds };
        }
        QNode::Internal { mass, cx, cy, children, bounds: b } => {
            // Running center-of-mass update (Welford-style incremental mean).
            let new_mass = *mass + 1.0;
            *cx = (*cx * *mass + x) / new_mass;
            *cy = (*cy * *mass + y) / new_mass;
            *mass = new_mass;
            let q = b.quadrant_of(x, y);
            let qb = b.quadrant(q);
            insert(&mut children[q], qb, idx, x, y, depth + 1);
        }
    }
}

fn accumulate_repulsion(
    node: &QNode,
    self_idx: usize,
    x: f64,
    y: f64,
    cfg: &ForceConfig,
    fx: &mut f64,
    fy: &mut f64,
) {
    match node {
        QNode::Empty => {}
        QNode::Leaf { idx, x: ox, y: oy } => {
            if *idx == self_idx {
                return;
            }
            let (dx, dy, dist) = delta(x, y, *ox, *oy);
            // FR repulsion kernel: k^2 / d.
            let f = (cfg.k * cfg.k) / dist;
            *fx += dx / dist * f;
            *fy += dy / dist * f;
        }
        QNode::Internal { mass, cx, cy, children, bounds } => {
            let (dx, dy, dist) = delta(x, y, *cx, *cy);
            // Barnes-Hut criterion: cell size / distance < theta => treat as
            // one far-field point mass instead of recursing into children.
            if (bounds.half * 2.0) / dist < cfg.theta {
                // A supernode of `mass` aggregated bodies repels like `mass`
                // independent k^2/d contributions from its center of mass.
                let f = (cfg.k * cfg.k) * mass / dist;
                *fx += dx / dist * f;
                *fy += dy / dist * f;
            } else {
                for child in children.iter() {
                    accumulate_repulsion(child, self_idx, x, y, cfg, fx, fy);
                }
            }
        }
    }
}

fn delta(x: f64, y: f64, ox: f64, oy: f64) -> (f64, f64, f64) {
    let dx = x - ox;
    let dy = y - oy;
    // Softened distance floor: two coincident/very-close bodies must not
    // divide-by-near-zero and eject to infinity.
    let dist = (dx * dx + dy * dy).sqrt().max(0.01);
    (dx, dy, dist)
}

fn query_node(node: &QNode, x: f64, y: f64, max_r: f64, best: &mut Option<(usize, f64)>) {
    match node {
        QNode::Empty => {}
        QNode::Leaf { idx, x: ox, y: oy } => {
            let d = ((x - ox).powi(2) + (y - oy).powi(2)).sqrt();
            if d <= max_r && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                *best = Some((*idx, d));
            }
        }
        QNode::Internal { children, bounds, .. } => {
            // Prune: if the click point is further than max_r outside this
            // cell's box (expanded by max_r), no descendant can be in range.
            let expanded = bounds.half + max_r;
            if (x - bounds.cx).abs() > expanded || (y - bounds.cy).abs() > expanded {
                return;
            }
            for child in children.iter() {
                query_node(child, x, y, max_r, best);
            }
        }
    }
}

/// Result of a completed simulation: final positions (index-aligned with the
/// input node count), the quadtree over those final positions (for hit
/// testing), and measurement fields for the spike's honesty report.
pub struct SimResult {
    pub positions: Vec<(f64, f64)>,
    pub tree: QuadTree,
    pub ticks_run: usize,
}

/// A live, steppable simulation — the SAME physics as [`simulate`], driven
/// one tick at a time so a caller (the code-graph view's animation frame,
/// via `code_graph_layout::LayoutDriver`) can slice the layout across frames
/// instead of blocking the main thread on one long run.
///
/// DETERMINISM: every piece of mutable state lives in this struct, the RNG is
/// consumed entirely during seeding, and `step()` performs exactly the FP
/// operations the old uninterrupted loop did, in the same order. Slicing
/// changes WHEN ticks run, never WHAT they compute — N ticks across arbitrary
/// per-frame budgets are bit-identical to N ticks in one run.
pub struct Simulation {
    bodies: Vec<Body>,
    /// Cloned (not borrowed) so the simulation outlives the caller's edge
    /// slice without a lifetime — 49k pairs is ~0.8 MB, noise next to the
    /// per-tick quadtree churn.
    edges: Vec<(usize, usize)>,
    cfg: ForceConfig,
    /// `cfg.gravity` pre-scaled by node count (see [`Simulation::new`]).
    gravity: f64,
    initial_temperature: f64,
    /// Ticks executed so far — also the cooling-schedule index.
    tick: usize,
    /// Converged early (max displacement under `convergence_eps`).
    converged: bool,
}

impl Simulation {
    /// Seed the bodies and precompute the derived constants. Runs NO ticks —
    /// tick 0's positions are the seeded circle, which the view paints
    /// immediately (time-to-first-paint is one frame, not one layout).
    pub fn new(node_count: usize, edges: &[(usize, usize)], seed: u64, cfg: &ForceConfig) -> Self {
        let mut rng = SplitMix64::new(seed);
        // Seed on a circle scaled to node count (area ~ n), not all at one point —
        // Barnes-Hut repulsion from a shared origin is a degenerate 0/0 direction.
        let radius = (node_count as f64).sqrt() * cfg.k;
        let bodies: Vec<Body> = (0..node_count)
            .map(|_| {
                let angle = rng.next_f64(0.0, std::f64::consts::TAU);
                let r = rng.next_f64(0.0, radius);
                Body { x: r * angle.cos(), y: r * angle.sin() }
            })
            .collect();

        // Barnes-Hut repulsion on an isolated (or near-isolated) body is
        // approximately k^2 * node_count / d at range — a monopole pull from the
        // WHOLE rest of the graph. A fixed-per-body gravity constant can't
        // counter that: it stays flat while aggregate repulsion grows with n, so
        // low-degree nodes drift further out as the graph gets bigger (observed:
        // a 205-node module graph sent an outlier to ~5700 units while the
        // connected bulk sat in a ~300-unit clump). Scaling gravity BY node_count
        // cancels the n-dependence, giving an isolated node's equilibrium
        // distance (k / sqrt(gravity)) that no longer grows with graph size.
        let gravity = cfg.gravity * node_count as f64;

        Self {
            bodies,
            edges: edges.to_vec(),
            cfg: *cfg,
            gravity,
            initial_temperature: cfg.k * 2.0,
            tick: 0,
            converged: false,
        }
    }

    /// No more ticks will run: converged early, or `max_ticks` exhausted.
    pub fn is_done(&self) -> bool {
        self.converged || self.tick >= self.cfg.max_ticks
    }

    pub fn ticks_run(&self) -> usize {
        self.tick
    }

    /// Advance one tick. Returns `true` if a tick ran, `false` if the
    /// simulation was already done (further calls are no-ops). The tick body
    /// is the old `simulate` loop body verbatim — same quadtree rebuild, same
    /// force order, same temperature schedule, same convergence check.
    pub fn step(&mut self) -> bool {
        if self.is_done() {
            return false;
        }
        let node_count = self.bodies.len();
        let points: Vec<(f64, f64)> = self.bodies.iter().map(|b| (b.x, b.y)).collect();
        let tree = QuadTree::build(&points);

        let mut fx = vec![0.0; node_count];
        let mut fy = vec![0.0; node_count];

        for (i, b) in self.bodies.iter().enumerate() {
            let (rx, ry) = tree.repulsion_at(i, b.x, b.y, &self.cfg);
            fx[i] += rx;
            fy[i] += ry;
            // Centering gravity keeps a disconnected component from drifting
            // off to infinity under pure repulsion (see `gravity` above).
            fx[i] -= b.x * self.gravity;
            fy[i] -= b.y * self.gravity;
        }
        for &(a, b) in &self.edges {
            if a == b || a >= node_count || b >= node_count {
                continue;
            }
            // FR attraction kernel: d^2 / k.
            let (dx, dy, dist) = delta(self.bodies[b].x, self.bodies[b].y, self.bodies[a].x, self.bodies[a].y);
            let f = (dist * dist) / self.cfg.k;
            fx[a] += dx / dist * f;
            fy[a] += dy / dist * f;
            fx[b] -= dx / dist * f;
            fy[b] -= dy / dist * f;
        }

        // Classic FR cooling schedule: a per-tick displacement cap
        // ("temperature") that falls linearly to zero over `max_ticks`. This
        // is what keeps the simulation stable regardless of force magnitude —
        // a body can never jump further than the current temperature in one
        // tick, so early (large) forces from the random initial layout can't
        // fling anything to infinity, and late (small) temperature settles
        // the layout instead of jittering.
        let temperature =
            self.initial_temperature * (1.0 - self.tick as f64 / self.cfg.max_ticks as f64).max(0.0);
        let mut max_disp = 0.0_f64;
        for (i, b) in self.bodies.iter_mut().enumerate() {
            let disp = (fx[i] * fx[i] + fy[i] * fy[i]).sqrt().max(0.001);
            // Cap the move at the current temperature — direction from the
            // force, magnitude never exceeding what this tick's "heat" allows.
            let step = disp.min(temperature);
            b.x += fx[i] / disp * step;
            b.y += fy[i] / disp * step;
            max_disp = max_disp.max(step);
        }
        self.tick += 1;
        if max_disp < self.cfg.convergence_eps {
            self.converged = true;
        }
        true
    }

    /// Current positions, index-aligned with the input node count.
    pub fn positions(&self) -> Vec<(f64, f64)> {
        self.bodies.iter().map(|b| (b.x, b.y)).collect()
    }

    /// Current positions in the render upload layout (f32 pairs).
    pub fn positions_f32(&self) -> Vec<(f32, f32)> {
        self.bodies.iter().map(|b| (b.x as f32, b.y as f32)).collect()
    }

    /// Hit-test quadtree over the CURRENT positions. O(n log n) — callers
    /// building it per frame should reconsider; the view builds it once on
    /// settle (and once over the seeded positions for the pre-settle window).
    pub fn hit_tree(&self) -> QuadTree {
        QuadTree::build(&self.positions())
    }
}

/// Run the simulation to convergence (or `cfg.max_ticks`) in one blocking
/// call — the pre-Task-5 entry point, now a thin wrapper over [`Simulation`]
/// so tests and small-scale callers keep working unchanged. `edges` are
/// (from_index, to_index) pairs into `0..node_count`.
pub fn simulate(node_count: usize, edges: &[(usize, usize)], seed: u64, cfg: &ForceConfig) -> SimResult {
    if node_count == 0 {
        return SimResult { positions: Vec::new(), tree: QuadTree { root: QNode::Empty }, ticks_run: 0 };
    }
    let mut sim = Simulation::new(node_count, edges, seed, cfg);
    while sim.step() {}
    let positions = sim.positions();
    let tree = QuadTree::build(&positions);
    SimResult { positions, tree, ticks_run: sim.ticks_run() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulate_places_every_node_and_is_deterministic() {
        let edges = vec![(0, 1), (1, 2), (2, 0), (2, 3)];
        let cfg = ForceConfig { max_ticks: 50, ..ForceConfig::default() };
        let a = simulate(4, &edges, 42, &cfg);
        let b = simulate(4, &edges, 42, &cfg);
        assert_eq!(a.positions.len(), 4);
        assert_eq!(a.positions, b.positions, "same seed must reproduce the same layout");
    }

    #[test]
    fn connected_nodes_end_up_closer_than_a_disconnected_pair() {
        // 0-1 are connected by an edge; 2 has no edges at all. Attraction
        // should pull 0/1 together relative to an isolated body's typical
        // repulsion-only spacing.
        let edges = vec![(0, 1)];
        let cfg = ForceConfig { max_ticks: 200, ..ForceConfig::default() };
        let res = simulate(3, &edges, 7, &cfg);
        let d01 = ((res.positions[0].0 - res.positions[1].0).powi(2)
            + (res.positions[0].1 - res.positions[1].1).powi(2))
        .sqrt();
        let d02 = ((res.positions[0].0 - res.positions[2].0).powi(2)
            + (res.positions[0].1 - res.positions[2].1).powi(2))
        .sqrt();
        assert!(d01 < d02, "connected pair ({d01}) should be closer than the disconnected one ({d02})");
    }

    #[test]
    fn quadtree_hit_test_finds_the_nearest_point() {
        let points = [(0.0, 0.0), (100.0, 100.0), (5.0, 5.0)];
        let tree = QuadTree::build(&points);
        assert_eq!(tree.query_point(1.0, 1.0, 20.0), Some(0));
        assert_eq!(tree.query_point(6.0, 6.0, 20.0), Some(2));
        assert_eq!(tree.query_point(500.0, 500.0, 20.0), None, "nothing within radius");
    }

    // --- ≥1000-node interconnected-graph tests ------------------------------
    //
    // WHY ≥1000: every defect this feature shipped — single-column packing,
    // rotated aspect, dynamic-marker saturation — was invisible below ~50
    // nodes and obvious on real data. Small fixtures are not evidence here.

    /// Build a connected scale-free-ish graph: `n` nodes, each new node
    /// attaching to `m` earlier ones (preferential attachment), so the result
    /// has hubs and a realistic degree distribution rather than a uniform mesh.
    fn interconnected(n: usize, m: usize) -> (usize, Vec<(usize, usize)>) {
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

    /// Run the real `simulate` API with a fixed seed and the given tick
    /// budget, returning just the positions (what these tests care about).
    fn run_layout(node_count: usize, edges: &[(usize, usize)], max_ticks: usize) -> Vec<(f64, f64)> {
        let cfg = ForceConfig { max_ticks, ..ForceConfig::default() };
        simulate(node_count, edges, 42, &cfg).positions
    }

    /// Bounding-box width/height of a set of positions.
    fn extent(pos: &[(f64, f64)]) -> (f64, f64) {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(x, y) in pos {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        (max_x - min_x, max_y - min_y)
    }

    #[test]
    fn layout_is_stable_and_bounded_at_1000_interconnected_nodes() {
        let (n, edges) = interconnected(1000, 3);
        let pos = run_layout(n, &edges, 400);
        assert_eq!(pos.len(), n, "every node must be placed");
        for (i, (x, y)) in pos.iter().enumerate() {
            assert!(x.is_finite() && y.is_finite(), "node {i} has non-finite position");
        }
        let (w, h) = extent(&pos);
        // Both directions bounded: a one-directional check previously passed
        // while the layout was 44:1 the other way (rotated-aspect defect).
        let aspect = (w / h).max(h / w);
        assert!(aspect < 6.0, "aspect {aspect:.1} too extreme ({w:.0}x{h:.0})");
        assert!(w < 1.0e6 && h < 1.0e6, "layout exploded: {w:.0}x{h:.0}");
    }

    #[test]
    fn layout_is_deterministic_at_1000_nodes() {
        let (n, edges) = interconnected(1000, 3);
        let a = run_layout(n, &edges, 200);
        let b = run_layout(n, &edges, 200);
        assert_eq!(a, b, "same seed + same ticks must give identical positions");
    }

    #[test]
    fn isolated_nodes_do_not_fly_away() {
        // Regression: gravity must scale with node count, or degree-0 nodes
        // drift outward forever (observed in the spike).
        let (n, edges) = interconnected(1000, 3);
        let total = n + 50; // 50 nodes with no edges at all
        let pos = run_layout(total, &edges, 400);
        let (w, h) = extent(&pos);
        assert!(w < 1.0e5 && h < 1.0e5, "isolated nodes escaped: {w:.0}x{h:.0}");
    }
}
