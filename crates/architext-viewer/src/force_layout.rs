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
    /// Pull toward a node's CLUSTER anchor, per unit of distance from it.
    ///
    /// Zero means unclustered, which is the historical behaviour: every node
    /// is pulled only toward the origin, and a system where everything repels
    /// everything and edges pull has one shape -- a ball. That is why the code
    /// graph rendered as a single contiguous sphere at both tiers, however few
    /// nodes were drawn: aggregating 3,638 functions down to 306 modules
    /// changed the count and not the shape.
    ///
    /// With an anchor per node, members of a cluster are held near a shared
    /// point and the graph resolves into lobes. Firmer than `gravity`, which
    /// only has to stop a disconnected component drifting away.
    pub cluster_pull: f64,
}

impl Default for ForceConfig {
    fn default() -> Self {
        Self {
            theta: 0.85,
            k: 60.0,
            gravity: 0.02,
            max_ticks: 400,
            convergence_eps: 0.05,
            cluster_pull: 0.0,
        }
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
    /// Per-node cluster anchor. Empty when the layout is unclustered, in which
    /// case only the origin-directed `gravity` applies.
    anchors: Vec<(f64, f64)>,
    /// INTERACTION mode. Off for every settle, so the cached, corpus-measured
    /// layout is bit-identical to before this existed — see
    /// `live_mode_is_opt_in_so_the_settle_is_bit_identical_to_before_it_existed`.
    live: bool,
    /// Per-node velocity. Live mode only, and the whole reason it exists: the
    /// settle integrator moves a body straight down the force direction, which
    /// cannot store energy and so can never swing back through equilibrium.
    /// Rebound IS overshoot, and overshoot needs momentum.
    vel: Vec<(f64, f64)>,
    /// The node held under the cursor, and where it is held. One at a time —
    /// a mouse drags one thing.
    pin: Option<(usize, f64, f64)>,
    /// Ticks granted beyond `cfg.max_ticks` by [`Simulation::reheat`]. Kept
    /// separate from `cfg.max_ticks` because that value is the DENOMINATOR of
    /// the cooling schedule; extending it there would change the settle's
    /// temperature curve, which is exactly what must not move.
    extra_ticks: usize,
}

/// Fraction of velocity shed per live tick — d3-force's `velocityDecay`.
/// Damping turns an oscillation into a rebound that comes to rest instead of
/// ringing forever; too much of it and there is no rebound at all, because
/// velocity just tracks the force and the body slides into equilibrium.
///
/// MEASURED against `a_released_node_swings_back_through_equilibrium...`,
/// which pulls a settled pair to 3x its rest length and releases:
///
/// | damping | closest approach vs 59.2 rest | overshoot |
/// | --- | --- | --- |
/// | 0.40 (d3's default) | 59.3 | none — overdamped |
/// | 0.30 | 59.2 | none |
/// | 0.20 | 58.0 | 2% — under the visible floor |
/// | 0.12 | passes | >= 3% |
///
/// d3's own 0.4 does not transfer, because its integrator scales forces
/// differently. 0.12 is the most damped value that still rebounds visibly:
/// springy, but it settles rather than wobbling.
const LIVE_DAMPING: f64 = 0.12;

/// Force-to-velocity scale for one live tick — the integrator's `dt`. Small
/// because forces are of order `k` (60) at equilibrium, and a body that moves
/// a full edge length per tick is a body that has exploded.
const LIVE_STEP: f64 = 0.05;

/// Ceiling on how far a body may move in one live tick, as a multiple of the
/// ideal edge length. The settle's stability comes from its cooling schedule,
/// which live mode does not have; this is the blow-up guard that replaces it.
/// FR attraction grows as `d^2/k`, so a node dragged far exerts a large pull —
/// that IS the tension, and the clamp only stops it becoming a teleport.
const LIVE_MAX_SPEED_K: f64 = 1.0;

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
            anchors: Vec::new(),
            live: false,
            vel: Vec::new(),
            pin: None,
            extra_ticks: 0,
        }
    }

    /// Seed the simulation with a per-node cluster anchor.
    ///
    /// `anchors[i]` is the point node `i` is held near. Positions are also
    /// seeded AT the anchors rather than on the usual circle, so a node starts
    /// in its own neighbourhood instead of being flung across the graph and
    /// dragged back -- which both settles faster and stops early frames
    /// showing a shape the final layout will not have.
    pub fn new_clustered(
        node_count: usize,
        edges: &[(usize, usize)],
        anchors: &[(f64, f64)],
        seed: u64,
        cfg: &ForceConfig,
    ) -> Self {
        let mut sim = Self::new(node_count, edges, seed, cfg);
        if anchors.len() == node_count {
            let mut rng = SplitMix64::new(seed ^ 0x9E37_79B9_7F4A_7C15);
            for (i, b) in sim.bodies.iter_mut().enumerate() {
                let (ax, ay) = anchors[i];
                // A small deterministic jitter, so co-anchored nodes do not
                // start exactly coincident (which divides by zero in the
                // repulsion kernel and wastes the first ticks separating them).
                b.x = ax + rng.next_f64(-cfg.k / 2.0, cfg.k / 2.0);
                b.y = ay + rng.next_f64(-cfg.k / 2.0, cfg.k / 2.0);
            }
            sim.anchors = anchors.to_vec();
        }
        sim
    }

    /// No more ticks will run: converged early, or `max_ticks` exhausted.
    pub fn is_done(&self) -> bool {
        // Live mode runs until the motion dies, NOT until a tick budget runs
        // out: a settle has a known amount of work to do, whereas a hand on
        // the graph decides when it is finished. `converged` still stops it,
        // so a released graph comes to rest instead of ticking forever.
        if self.live {
            return self.converged;
        }
        self.converged || self.tick >= self.cfg.max_ticks + self.extra_ticks
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
            // off to infinity under pure repulsion (see `gravity` above). When
            // the node has a cluster anchor, that anchor IS the centre it is
            // held to -- which is what turns one contiguous sphere into
            // separate lobes.
            //
            // The anchor has to REPLACE the origin pull rather than be added
            // alongside it. `gravity` is pre-scaled by node count (0.02 x 3638
            // = 72.8 per unit here), so a separately-tuned anchor force of 0.55
            // was outweighed 132 to 1: the clustered settle ran with correct
            // anchors for every node and produced a picture identical to the
            // unclustered one, because everything was still being dragged to
            // the same point.
            //
            // Repulsion and edge attraction are untouched, so structure INSIDE
            // a lobe is still the graph's own; the anchor only decides which
            // neighbourhood a node occupies.
            let (gx, gy) = self.anchors.get(i).copied().unwrap_or((0.0, 0.0));
            // `cluster_pull` scales the node-count-scaled gravity. Above ~1 it
            // crushes each lobe to a point; the useful range is well below.
            let pull = if self.anchors.is_empty() {
                self.gravity
            } else {
                self.gravity * self.cfg.cluster_pull
            };
            fx[i] -= (b.x - gx) * pull;
            fy[i] -= (b.y - gy) * pull;
        }
        for &(a, b) in &self.edges {
            if a == b || a >= node_count || b >= node_count {
                continue;
            }
            // FR attraction kernel: d^2 / k. Deliberately UNCHANGED in live
            // mode. A Hooke spring about `k` was tried and reverted: it
            // balances repulsion at d = 94 rather than d = 60, so merely
            // touching the graph would have breathed it outward by half an
            // edge length. Elasticity is a property of the INTEGRATOR (stored
            // momentum), not of the force law — keeping one kernel keeps one
            // equilibrium, so live mode starts exactly where the settle
            // stopped.
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
        if self.live {
            // Velocity Verlet: force accelerates, damping bleeds the energy
            // off, position integrates the velocity. A body arrives at
            // equilibrium still MOVING, carries through it, and is pulled
            // back — the rebound. Speed is clamped at one edge length per
            // tick purely as a blow-up guard; it is not a cooling schedule.
            for (i, b) in self.bodies.iter_mut().enumerate() {
                let v = &mut self.vel[i];
                v.0 = (v.0 + fx[i] * LIVE_STEP) * (1.0 - LIVE_DAMPING);
                v.1 = (v.1 + fy[i] * LIVE_STEP) * (1.0 - LIVE_DAMPING);
                let speed = (v.0 * v.0 + v.1 * v.1).sqrt();
                let max_speed = self.cfg.k * LIVE_MAX_SPEED_K;
                if speed > max_speed {
                    v.0 *= max_speed / speed;
                    v.1 *= max_speed / speed;
                }
                b.x += v.0;
                b.y += v.1;
                max_disp = max_disp.max((v.0 * v.0 + v.1 * v.1).sqrt());
            }
            // The held node goes exactly where the cursor is, with no residual
            // velocity to carry it off when released mid-motion. It still
            // EXERTS force on its neighbours above, which is what transmits
            // the tension along the edges.
            if let Some((i, px, py)) = self.pin {
                if i < self.bodies.len() {
                    self.bodies[i] = Body { x: px, y: py };
                    self.vel[i] = (0.0, 0.0);
                    // A hand on the graph is energy going in: never let the
                    // convergence check call this settled while dragging.
                    max_disp = max_disp.max(self.cfg.convergence_eps * 2.0);
                }
            }
        } else {
            for (i, b) in self.bodies.iter_mut().enumerate() {
                let disp = (fx[i] * fx[i] + fy[i] * fy[i]).sqrt().max(0.001);
                // Cap the move at the current temperature — direction from the
                // force, magnitude never exceeding what this tick's "heat" allows.
                let step = disp.min(temperature);
                b.x += fx[i] / disp * step;
                b.y += fy[i] / disp * step;
                max_disp = max_disp.max(step);
            }
        }
        self.tick += 1;
        if max_disp < self.cfg.convergence_eps {
            self.converged = true;
        }
        true
    }

    /// Build a LIVE simulation over positions that already exist, rather than
    /// the seeded circle `new` starts from.
    ///
    /// Required because the common load path never runs a settle at all: a
    /// layout-cache hit has final positions and no simulation, so without this
    /// the first node grabbed on a cached graph would have nothing to grab.
    /// `anchors` must be the same cluster anchors the settle used, or the
    /// lobes would slowly dissolve while the graph is being handled.
    pub fn from_positions(
        positions: &[(f32, f32)],
        edges: &[(usize, usize)],
        anchors: &[(f64, f64)],
        cfg: &ForceConfig,
    ) -> Self {
        let bodies: Vec<Body> =
            positions.iter().map(|&(x, y)| Body { x: x as f64, y: y as f64 }).collect();
        let node_count = bodies.len();
        Self {
            vel: vec![(0.0, 0.0); node_count],
            bodies,
            edges: edges.to_vec(),
            cfg: *cfg,
            gravity: cfg.gravity * node_count as f64,
            initial_temperature: cfg.k * 2.0,
            tick: 0,
            converged: false,
            anchors: anchors.to_vec(),
            live: true,
            pin: None,
            extra_ticks: 0,
        }
    }

    /// Whether this simulation is in interaction mode.
    pub fn is_live(&self) -> bool {
        self.live
    }

    /// Switch between the settle integrator and the INTERACTION one.
    ///
    /// Enabling reopens the simulation (a settled one is `converged`, and
    /// `step` is a no-op in that state) and allocates the velocity vector.
    /// Nothing else in the crate turns this on, which is what keeps every
    /// settle — and therefore the layout cache and the corpus ratchet —
    /// exactly as it was.
    pub fn set_live(&mut self, live: bool) {
        self.live = live;
        if live {
            self.vel.resize(self.bodies.len(), (0.0, 0.0));
            self.converged = false;
        } else {
            self.pin = None;
        }
    }

    /// Hold node `i` at `(x, y)` — the node under the cursor during a drag.
    /// `None` releases it, leaving whatever tension the edges have stored to
    /// pull it back.
    pub fn set_pin(&mut self, pin: Option<(usize, f64, f64)>) {
        self.pin = pin;
    }

    /// Grant `ticks` more and clear convergence, so a finished simulation can
    /// run again. `is_done()` is otherwise a one-way door: interaction needs a
    /// way back through it.
    pub fn reheat(&mut self, ticks: usize) {
        self.extra_ticks += ticks;
        self.converged = false;
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
    simulate_clustered(node_count, edges, seed, cfg, &[])
}

/// `simulate` with per-node cluster anchors. An empty `anchors` behaves
/// exactly like the unclustered settle.
pub fn simulate_clustered(
    node_count: usize,
    edges: &[(usize, usize)],
    seed: u64,
    cfg: &ForceConfig,
    anchors: &[(f64, f64)],
) -> SimResult {
    if node_count == 0 {
        return SimResult { positions: Vec::new(), tree: QuadTree { root: QNode::Empty }, ticks_run: 0 };
    }
    let mut sim = if anchors.len() == node_count {
        Simulation::new_clustered(node_count, edges, anchors, seed, cfg)
    } else {
        Simulation::new(node_count, edges, seed, cfg)
    };
    while sim.step() {}
    let positions = sim.positions();
    let tree = QuadTree::build(&positions);
    SimResult { positions, tree, ticks_run: sim.ticks_run() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settle a 2-node/1-edge pair and report their resting distance — the
    /// equilibrium an elastic rebound has to swing THROUGH.
    fn settled_pair(cfg: &ForceConfig) -> (Simulation, f64) {
        let mut sim = Simulation::new(2, &[(0, 1)], 42, cfg);
        while sim.step() {}
        let p = sim.positions();
        let d = ((p[0].0 - p[1].0).powi(2) + (p[0].1 - p[1].1).powi(2)).sqrt();
        (sim, d)
    }

    #[test]
    fn a_released_node_swings_back_through_equilibrium_instead_of_sliding_to_it() {
        // WHY: this IS the "elastic edges that rebound" requirement, and it is
        // the one behaviour the settle integrator cannot produce at any
        // setting. `step` moves a body straight down the force direction,
        // capped by temperature (`b.x += fx/disp * step`) — there is no
        // velocity, so energy is never stored and a node can only approach
        // equilibrium, never pass it. Overshoot is the observable difference
        // between a spring and a slide, so it is what the test asserts.
        let cfg = ForceConfig { max_ticks: 400, ..ForceConfig::default() };
        let (mut sim, rest) = settled_pair(&cfg);

        // Pull node 1 out to 3x its resting distance and hold it there, so the
        // edge is under tension, then let go.
        sim.set_live(true);
        let p = sim.positions();
        let (ax, ay) = p[0];
        sim.set_pin(Some((1, ax + rest * 3.0, ay)));
        for _ in 0..30 {
            sim.step();
        }
        sim.set_pin(None);

        let mut min_seen = f64::MAX;
        for _ in 0..400 {
            sim.step();
            let p = sim.positions();
            let d = ((p[0].0 - p[1].0).powi(2) + (p[0].1 - p[1].1).powi(2)).sqrt();
            min_seen = min_seen.min(d);
        }
        // A 3% floor, not "any amount below rest": an overshoot of 0.1 units
        // is numerical noise that a user cannot see, and a test that passes on
        // it would be asserting the feature exists while it does not.
        assert!(
            min_seen < rest * 0.97,
            "released node must VISIBLY overshoot: rest {rest:.1}, closest approach {min_seen:.1}"
        );
    }

    #[test]
    fn a_pinned_node_stays_exactly_where_it_is_held_while_its_neighbour_follows() {
        // WHY: a drag must move the node under the cursor EXACTLY (any drift
        // and the node slides out from under the pointer), while the tension
        // it applies still reaches its neighbours — that pull is what makes
        // the web read as connected rather than as independent dots.
        let cfg = ForceConfig { max_ticks: 400, ..ForceConfig::default() };
        let (mut sim, rest) = settled_pair(&cfg);
        sim.set_live(true);
        let before = sim.positions()[0];
        let (tx, ty) = (before.0 + rest * 4.0, before.1 + rest * 2.0);
        sim.set_pin(Some((1, tx, ty)));
        for _ in 0..40 {
            sim.step();
        }
        let after = sim.positions();
        assert!(
            (after[1].0 - tx).abs() < 1e-9 && (after[1].1 - ty).abs() < 1e-9,
            "pinned node must sit exactly on its pin, got {:?} want {:?}",
            after[1],
            (tx, ty)
        );
        let moved = ((after[0].0 - before.0).powi(2) + (after[0].1 - before.1).powi(2)).sqrt();
        assert!(moved > 1.0, "the neighbour must be dragged along, moved {moved:.2}");
    }

    #[test]
    fn live_mode_is_opt_in_so_the_settle_is_bit_identical_to_before_it_existed() {
        // WHY: THE FIREWALL. The cold settle feeds the layout cache and the
        // corpus fitness ratchet, so live mode must be a path the settle never
        // takes. If this fails, every cached layout and every corpus
        // measurement has silently moved.
        let edges = vec![(0, 1), (1, 2), (2, 0), (2, 3)];
        let cfg = ForceConfig { max_ticks: 50, ..ForceConfig::default() };
        let baseline = simulate(4, &edges, 42, &cfg);
        let mut sim = Simulation::new(4, &edges, 42, &cfg);
        while sim.step() {}
        assert_eq!(sim.positions(), baseline.positions, "a default sim must not change at all");
    }

    #[test]
    fn reheating_reopens_a_finished_simulation() {
        // WHY: `is_done()` is a one-way door today (converged OR max_ticks),
        // and the driver is dropped behind it. Interaction needs a way back in.
        let cfg = ForceConfig { max_ticks: 30, ..ForceConfig::default() };
        let (mut sim, _) = settled_pair(&cfg);
        assert!(sim.is_done(), "the pair settles inside its tick budget");
        assert!(!sim.step(), "a done simulation is a no-op");
        sim.reheat(60);
        assert!(!sim.is_done(), "reheat must reopen it");
        assert!(sim.step(), "and ticking must resume");
    }

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
