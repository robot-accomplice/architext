//! Forbidden-artifact gate for the deterministic router.
//!
//! The §0 law defines a set of artifacts that are NEVER permissible: a **dogleg**
//! (a line that folds back over itself), a **Z / staircase** (a jog whose end
//! segments sit on opposite sides of the middle), a **channel overlap** (two lines
//! sharing a run, incl. coincident lines), a **non-orthogonal** segment (lines may
//! meet only orthogonally, so every segment must be axis-aligned), an **unrouted**
//! edge, and a **min-stem violation** (a route that bends right at the wall instead
//! of travelling ≥ [`MIN_SURFACE_STEM`] straight off the surface first).
//!
//! [`audit_routes`] detects each class on one routing. The routing is VALID only if
//! the gate finds NONE of them; the corpus harness (`audit_model` binary) then —
//! and only then — validates everything else (shape legality, monotonicity, β,
//! crossings, length).

use super::{
    bend_score, corners, doubles_back, segments_overlap, DOGLEG_PENALTY, EPS, MIN_SURFACE_STEM,
    Z_PENALTY,
};
use crate::model::Point;

/// Per-routing forbidden-artifact gate plus the soft (post-gate) validation facts.
#[derive(Default, Debug)]
pub struct RouteAudit {
    // ---- FORBIDDEN classes (every one must be empty for the gate to pass) ----
    /// Routes that double back over themselves (edge indices).
    pub doglegs: Vec<usize>,
    /// Z jogs (2-bend, end segments on opposite sides) or ≥3-bend staircases.
    pub staircases: Vec<usize>,
    /// Routes with a diagonal (non-axis-aligned) segment.
    pub non_orthogonal: Vec<usize>,
    /// Edges the model could not route (empty / single-point polyline).
    pub unrouted: Vec<usize>,
    /// Routes that bend before clearing [`MIN_SURFACE_STEM`] off a mount.
    pub short_stems: Vec<usize>,
    /// Pairs of routes whose segments share a channel (collinear overlap).
    pub channel_overlaps: Vec<(usize, usize)>,
    // ---- TRACK-MODEL TARGET (tracked metric, not yet gate-blocking) ----
    /// Pairs of routes whose parallel runs are closer than [`MIN_CHANNEL_CLEARANCE`]
    /// (an arrowhead) without overlapping — the "channel buffer" rule. Currently
    /// unavoidable on over-capacity faces; the track model drives this to 0.
    pub tight_channels: Vec<(usize, usize)>,
    // ---- SOFT validation (only meaningful once the gate is clean) ----
    pub straight: usize,
    pub ells: usize,
    pub cees: usize,
    pub beta: f64,
    pub crossings: usize,
    pub length: f64,
}

impl RouteAudit {
    /// Total forbidden-artifact instances across every class.
    pub fn forbidden(&self) -> usize {
        self.doglegs.len()
            + self.staircases.len()
            + self.non_orthogonal.len()
            + self.unrouted.len()
            + self.short_stems.len()
            + self.channel_overlaps.len()
    }

    /// True iff the gate found NO forbidden artifacts.
    pub fn is_clean(&self) -> bool {
        self.forbidden() == 0
    }
}

fn seg_len(a: &Point, b: &Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Minimum clearance between two parallel routes — "a channel an arrowhead wide".
/// The flow arrowhead marker is ~7px (`markerWidth/Height="7"` in the renderer);
/// parallel runs closer than this share visual space (the channel-buffer rule).
pub const MIN_CHANNEL_CLEARANCE: f64 = 7.0;

/// Do two axis-aligned segments run PARALLEL and overlap along their shared axis
/// closer than `clearance` (perpendicular gap in `(EPS, clearance)`)? Exact overlap
/// (gap ≈ 0) is excluded — that is the `channel_overlaps` case, not a buffer breach.
fn parallel_too_close(a0: &Point, a1: &Point, b0: &Point, b1: &Point, clearance: f64) -> bool {
    let a_h = (a0.y - a1.y).abs() < EPS;
    let b_h = (b0.y - b1.y).abs() < EPS;
    let a_v = (a0.x - a1.x).abs() < EPS;
    let b_v = (b0.x - b1.x).abs() < EPS;
    if a_h && b_h {
        let gap = (a0.y - b0.y).abs();
        if gap <= EPS || gap >= clearance {
            return false;
        }
        let lo = a0.x.min(a1.x).max(b0.x.min(b1.x));
        let hi = a0.x.max(a1.x).min(b0.x.max(b1.x));
        lo + EPS < hi
    } else if a_v && b_v {
        let gap = (a0.x - b0.x).abs();
        if gap <= EPS || gap >= clearance {
            return false;
        }
        let lo = a0.y.min(a1.y).max(b0.y.min(b1.y));
        let hi = a0.y.max(a1.y).min(b0.y.max(b1.y));
        lo + EPS < hi
    } else {
        false
    }
}

/// Audit one routing for every forbidden artifact, plus the soft shape facts.
pub fn audit_routes(routes: &[Vec<Point>]) -> RouteAudit {
    let mut a = RouteAudit::default();
    for (i, r) in routes.iter().enumerate() {
        if r.len() < 2 {
            a.unrouted.push(i);
            continue;
        }
        // non-orthogonal: any diagonal (both x and y change) segment.
        if r.windows(2).any(|w| (w[0].x - w[1].x).abs() > EPS && (w[0].y - w[1].y).abs() > EPS) {
            a.non_orthogonal.push(i);
        }
        // dogleg vs Z/staircase via the §0 shape classifier.
        let score = bend_score(r);
        if doubles_back(r) || score >= DOGLEG_PENALTY {
            a.doglegs.push(i);
        } else if (score - Z_PENALTY).abs() < EPS {
            a.staircases.push(i); // Z (2-bend, opposite sides) or ≥3-bend staircase
        } else {
            // a legal shape — record it and check its stems.
            match score as i64 {
                0 => a.straight += 1,
                1 => a.ells += 1,
                _ => a.cees += 1,
            }
            // min-stem: the segment off each mount before the first/last bend.
            let c = corners(r);
            if c.len() >= 3 {
                let first = seg_len(&c[0], &c[1]);
                let last = seg_len(&c[c.len() - 1], &c[c.len() - 2]);
                if first + EPS < MIN_SURFACE_STEM || last + EPS < MIN_SURFACE_STEM {
                    a.short_stems.push(i);
                }
            }
        }
        a.beta += score;
        a.length += r.windows(2).map(|w| seg_len(&w[0], &w[1])).sum::<f64>();
    }
    // channel overlaps: pairs of routes sharing a collinear run (incl. coincident).
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            let shares = routes[i].windows(2).any(|wi| {
                routes[j]
                    .windows(2)
                    .any(|wj| segments_overlap(&wi[0], &wi[1], &wj[0], &wj[1]))
            });
            if shares {
                a.channel_overlaps.push((i, j));
            }
            // channel buffer: parallel runs closer than an arrowhead (but not the
            // exact-overlap case above).
            let tight = routes[i].windows(2).any(|wi| {
                routes[j].windows(2).any(|wj| {
                    parallel_too_close(&wi[0], &wi[1], &wj[0], &wj[1], MIN_CHANNEL_CLEARANCE)
                })
            });
            if tight {
                a.tight_channels.push((i, j));
            }
        }
    }
    a.crossings = super::place::total_crossings(routes);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn flags_a_dogleg() {
        // right to x=20 then BACK left to x=10 (overlaps itself on y=0) then up.
        let dogleg = vec![p(0.0, 0.0), p(20.0, 0.0), p(10.0, 0.0), p(10.0, 20.0)];
        let a = audit_routes(&[dogleg]);
        assert_eq!(a.doglegs, vec![0], "dogleg must be flagged");
        assert!(!a.is_clean());
    }

    #[test]
    fn flags_a_z_jog() {
        // end segments point the SAME way but sit on OPPOSITE sides of the vertical
        // middle (x=20): (0,0) is left of it, (40,10) is right of it → a Z.
        let z = vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(40.0, 10.0)];
        let a = audit_routes(&[z]);
        assert_eq!(a.staircases, vec![0], "Z jog must be flagged");
        assert!(!a.is_clean());
    }

    #[test]
    fn flags_a_non_orthogonal_segment() {
        let diag = vec![p(0.0, 0.0), p(10.0, 10.0)];
        let a = audit_routes(&[diag]);
        assert_eq!(a.non_orthogonal, vec![0], "diagonal segment must be flagged");
    }

    #[test]
    fn flags_an_unrouted_edge() {
        let a = audit_routes(&[Vec::new()]);
        assert_eq!(a.unrouted, vec![0]);
    }

    #[test]
    fn flags_a_short_stem() {
        // L whose first stem is only 5 < MIN_SURFACE_STEM (16) — bends at the wall.
        let l = vec![p(0.0, 0.0), p(0.0, 5.0), p(40.0, 5.0)];
        let a = audit_routes(&[l]);
        assert_eq!(a.short_stems, vec![0], "stem below MIN_SURFACE_STEM must be flagged");
    }

    #[test]
    fn flags_a_channel_overlap() {
        // two vertical routes on x=0 sharing the run y[5,10].
        let r0 = vec![p(0.0, 0.0), p(0.0, 10.0)];
        let r1 = vec![p(0.0, 5.0), p(0.0, 15.0)];
        let a = audit_routes(&[r0, r1]);
        assert_eq!(a.channel_overlaps, vec![(0, 1)], "shared channel must be flagged");
    }

    #[test]
    fn flags_close_parallel_channels_under_an_arrowhead() {
        // two horizontal runs 4px apart, overlapping in x → closer than an arrowhead
        // (no exact overlap, so channel_overlaps stays empty; tight_channels catches it).
        let r0 = vec![p(0.0, 100.0), p(50.0, 100.0)];
        let r1 = vec![p(10.0, 104.0), p(60.0, 104.0)];
        let a = audit_routes(&[r0, r1]);
        assert_eq!(a.channel_overlaps, Vec::<(usize, usize)>::new(), "not an exact overlap");
        assert_eq!(a.tight_channels, vec![(0, 1)], "4px < arrowhead must be flagged");
    }

    #[test]
    fn parallel_runs_an_arrowhead_apart_are_not_flagged() {
        let r0 = vec![p(0.0, 100.0), p(50.0, 100.0)];
        let r1 = vec![p(10.0, 100.0 + MIN_CHANNEL_CLEARANCE + 1.0), p(60.0, 100.0 + MIN_CHANNEL_CLEARANCE + 1.0)];
        let a = audit_routes(&[r0, r1]);
        assert!(a.tight_channels.is_empty(), "≥ arrowhead apart is fine: {a:?}");
    }

    #[test]
    fn clean_routing_passes_the_gate_and_counts_shapes() {
        // straight, L (stems 20≥16), C arch (stems 20≥16) — all legal §0 shapes.
        let straight = vec![p(0.0, 0.0), p(40.0, 0.0)];
        let ell = vec![p(0.0, 100.0), p(0.0, 120.0), p(40.0, 120.0)];
        let cee = vec![p(0.0, 200.0), p(0.0, 180.0), p(40.0, 180.0), p(40.0, 200.0)];
        let a = audit_routes(&[straight, ell, cee]);
        assert!(a.is_clean(), "no forbidden artifacts: {a:?}");
        assert_eq!((a.straight, a.ells, a.cees), (1, 1, 1));
    }
}
