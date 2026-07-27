//! Progressive layout driver (Plan C Task 5) — pure Rust, zero web-sys.
//!
//! Wraps [`force_layout::Simulation`] so the code-graph view can advance the
//! Barnes-Hut layout in per-frame slices (progressive ticking) instead of one
//! blocking `simulate` call: each animation frame spends a millisecond budget
//! running ticks, then re-uploads positions and paints, so the user WATCHES
//! the graph settle and the tab never freezes. The measured 6.5–27 s
//! main-thread block at 17,561 nodes becomes ~6.5–27 s of responsive frames.
//!
//! Why a clock-based budget rather than a fixed ticks-per-frame count: a tick
//! at the module tier costs microseconds, at the function tier tens of
//! milliseconds. A fixed count either starves the big tier (1 tick/frame) or
//! re-introduces a multi-second block on the first frame (400 ticks/frame).
//! The budget self-adjusts: small tiers settle in one frame, the big tier
//! gets ~1 tick/frame and stays interactive.
//!
//! DETERMINISM: slicing changes WHEN ticks run, never WHAT they compute —
//! [`Simulation`] carries all state between `step()` calls and the RNG is
//! consumed entirely at seeding, so N ticks across arbitrary per-frame
//! budgets are bit-identical to N ticks in one run. Proven at 1000+ nodes in
//! the tests below.
use crate::force_layout::{ForceConfig, QuadTree, Simulation};

/// The driver for one tier's layout. Owns the simulation; the view owns the
/// clock and the per-frame budget.
pub struct LayoutDriver {
    sim: Simulation,
    max_ticks: usize,
}

impl LayoutDriver {
    /// Seed a layout (runs no ticks — tick-0 positions are the seeded circle
    /// the view paints on the very first frame).
    pub fn new(node_count: usize, edges: &[(usize, usize)], seed: u64, cfg: &ForceConfig) -> Self {
        Self { sim: Simulation::new(node_count, edges, seed, cfg), max_ticks: cfg.max_ticks }
    }

    /// Advance the layout until it is done or `now()` reports more than
    /// `budget_ms` elapsed — but ALWAYS at least one tick per call, so a tick
    /// slower than the budget can't starve progress. Returns `true` when the
    /// layout has settled (converged or `max_ticks` reached).
    pub fn step_within(&mut self, budget_ms: f64, now: impl Fn() -> f64) -> bool {
        let t0 = now();
        loop {
            if !self.sim.step() {
                return true;
            }
            if now() - t0 >= budget_ms {
                return self.sim.is_done();
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.sim.is_done()
    }

    pub fn ticks_run(&self) -> usize {
        self.sim.ticks_run()
    }

    pub fn max_ticks(&self) -> usize {
        self.max_ticks
    }

    /// Current positions in the render upload layout (f32 pairs) — refreshed
    /// by the view after every slice so the settle is visible.
    pub fn positions_f32(&self) -> Vec<(f32, f32)> {
        self.sim.positions_f32()
    }

    /// Hit-test quadtree over the CURRENT positions. O(n log n) — the view
    /// builds this once over the seeded positions and once on settle, never
    /// per frame.
    pub fn hit_tree(&self) -> QuadTree {
        self.sim.hit_tree()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_graph_graph::tests_support;

    // --- ≥1000-node interconnected-graph tests ------------------------------
    //
    // WHY ≥1000 (maintainer requirement): every defect this feature shipped
    // was invisible below ~50 nodes. The slicing-determinism guarantee is the
    // load-bearing claim of Task 5, so it is proven at real scale, not on a
    // toy fixture.

    /// Scriptable fake clock: each closure call pops the next reading.
    fn fake_clock(readings: Vec<f64>) -> impl Fn() -> f64 {
        let cell = std::cell::RefCell::new(readings.into_iter());
        move || cell.borrow_mut().next().unwrap_or(f64::MAX)
    }

    /// Drive a layout to completion in irregular per-frame slices whose
    /// budgets are all over the place (sub-tick, multi-tick, exact-boundary).
    fn run_sliced(node_count: usize, edges: &[(usize, usize)], budgets_ms: &[f64]) -> Vec<(f32, f32)> {
        let cfg = ForceConfig::default();
        let mut d = LayoutDriver::new(node_count, edges, 42, &cfg);
        let mut i = 0;
        while !d.is_done() {
            let budget = budgets_ms[i % budgets_ms.len()];
            i += 1;
            // Fake clock: t0 = 0, then one reading per post-tick check. To
            // run K ticks in a slice, feed K-1 "under budget" readings then
            // one "over budget" reading. Sub-tick budgets (0.0) still run
            // exactly one tick — the at-least-one-tick guarantee.
            let ticks_this_slice = (budget / 10.0).ceil().max(1.0) as usize;
            let mut readings = vec![0.0];
            for k in 0..ticks_this_slice {
                readings.push(if k + 1 < ticks_this_slice { 5.0 * (k as f64 + 1.0) } else { budget + 1.0 });
            }
            d.step_within(budget, fake_clock(readings));
        }
        d.positions_f32()
    }

    #[test]
    fn slicing_does_not_change_the_layout_at_1000_nodes() {
        let (n, edges) = tests_support::interconnected(1000, 3);
        let cfg = ForceConfig::default();
        let uninterrupted = crate::force_layout::simulate(n, &edges, 42, &cfg);
        let expected: Vec<(f32, f32)> =
            uninterrupted.positions.iter().map(|&(x, y)| (x as f32, y as f32)).collect();

        // Irregular slice pattern: sub-tick slices, 1-tick slices, big slices.
        let sliced = run_sliced(n, &edges, &[0.0, 1.0, 10.0, 37.0, 10.0, 1000.0, 3.0]);
        assert_eq!(sliced.len(), n, "every node must be placed");
        assert_eq!(
            sliced, expected,
            "400 ticks across arbitrary per-frame budgets must be bit-identical to 400 uninterrupted ticks"
        );
    }

    #[test]
    fn sliced_run_executes_the_same_number_of_ticks_at_1000_nodes() {
        let (n, edges) = tests_support::interconnected(1000, 3);
        let cfg = ForceConfig::default();
        let uninterrupted = crate::force_layout::simulate(n, &edges, 42, &cfg);

        let mut d = LayoutDriver::new(n, &edges, 42, &cfg);
        // One tick per "frame" (budget 0 forces the at-least-one path) — the
        // most hostile slicing there is.
        while !d.step_within(0.0, fake_clock(vec![0.0, 1.0])) {}
        assert_eq!(
            d.ticks_run(),
            uninterrupted.ticks_run,
            "convergence must trigger on the same tick regardless of slicing"
        );
    }

    #[test]
    fn at_least_one_tick_per_slice_even_with_a_zero_budget() {
        let (n, edges) = tests_support::interconnected(1000, 3);
        let cfg = ForceConfig { max_ticks: 10, ..ForceConfig::default() };
        let mut d = LayoutDriver::new(n, &edges, 42, &cfg);
        // Clock that is ALWAYS over budget: exactly one tick must still run.
        let done = d.step_within(0.0, || 1.0e9);
        assert!(!done, "10-tick budget cannot be done after one tick");
        assert_eq!(d.ticks_run(), 1, "a zero/negative budget must not starve progress");
    }
}
