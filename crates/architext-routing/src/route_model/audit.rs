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
    bend_score, corners, doubles_back, is_stemmed_jog, segments_overlap, DOGLEG_PENALTY, EPS, MIN_SURFACE_STEM,
    Z_PENALTY,
};
use crate::model::{Point, Rect};

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

/// Whether segment `a→b` runs **flush** to one of `rect`'s faces: it lies on a
/// face line (collinear, within [`EPS`]) AND overlaps that face's extent for a
/// nonzero length. A perpendicular stem only TOUCHES its mount face at a point
/// (zero overlap along the face), so it is not flush; a segment that runs ALONG a
/// surface is — the verboten "L with a side flush to a surface". Checked against
/// EVERY node (incl. the route's own endpoints, which is how a grazing stem is
/// caught).
fn segment_flush_to_rect(a: &Point, b: &Point, rect: &Rect) -> bool {
    let horizontal = (a.y - b.y).abs() < EPS;
    let vertical = (a.x - b.x).abs() < EPS;
    if horizontal {
        let (lo, hi) = (a.x.min(b.x).max(rect.x), a.x.max(b.x).min(rect.x + rect.width));
        if lo + EPS < hi {
            return (a.y - rect.y).abs() < EPS || (a.y - (rect.y + rect.height)).abs() < EPS;
        }
    } else if vertical {
        let (lo, hi) = (a.y.min(b.y).max(rect.y), a.y.max(b.y).min(rect.y + rect.height));
        if lo + EPS < hi {
            return (a.x - rect.x).abs() < EPS || (a.x - (rect.x + rect.width)).abs() < EPS;
        }
    }
    false
}

/// Minimum clearance between two parallel routes — "a channel an arrowhead wide".
/// The flow arrowhead marker is ~7px (`markerWidth/Height="7"` in the renderer);
/// parallel runs closer than this share visual space (the channel-buffer rule).
pub const MIN_CHANNEL_CLEARANCE: f64 = 7.0;

/// Do two axis-aligned segments run PARALLEL and overlap along their shared axis
/// closer than `clearance` (perpendicular gap in `(EPS, clearance)`)? Exact overlap
/// (gap ≈ 0) is excluded — that is the `channel_overlaps` case, not a buffer breach.
pub(crate) fn parallel_too_close(a0: &Point, a1: &Point, b0: &Point, b1: &Point, clearance: f64) -> bool {
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

/// Count route pairs whose parallel runs are closer than [`MIN_CHANNEL_CLEARANCE`]
/// (an arrowhead) without exactly overlapping — the channel-buffer breach count. Used
/// by the channel-separation guard to drive this toward zero.
pub(crate) fn total_tight_pairs(routes: &[Vec<Point>]) -> usize {
    let mut n = 0usize;
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            let tight = routes[i].windows(2).any(|wi| {
                routes[j].windows(2).any(|wj| {
                    parallel_too_close(&wi[0], &wi[1], &wj[0], &wj[1], MIN_CHANNEL_CLEARANCE)
                })
            });
            if tight {
                n += 1;
            }
        }
    }
    n
}

/// Audit one routing for every forbidden artifact, plus the soft shape facts.
/// `rects` are the node rectangles; pass an empty slice to skip the flush-to-surface
/// check (e.g. in pure-geometry unit tests with no layout).
pub fn audit_routes(routes: &[Vec<Point>], rects: &[Rect]) -> RouteAudit {
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
        let c = corners(r);
        // STEM EXCEPTION: a 2-bend facing jog whose end segments are proper stems is
        // the unavoidable, legal shape for an offset facing pair — stems are the sole
        // exception to the Z rule. It scores Z_PENALTY in the cost model but is NOT a
        // forbidden artifact. (A short-stem jog / ≥3-bend staircase stays forbidden.)
        let stemmed_jog = c.len() == 4 && is_stemmed_jog(&c);
        if doubles_back(r) || score >= DOGLEG_PENALTY {
            a.doglegs.push(i);
        } else if (score - Z_PENALTY).abs() < EPS && !stemmed_jog {
            a.staircases.push(i); // Z (2-bend, opposite sides) or ≥3-bend staircase
        } else {
            // a legal shape (incl. the stemmed facing jog) — record it, check stems.
            match score as i64 {
                0 => a.straight += 1,
                1 => a.ells += 1,
                _ => a.cees += 1,
            }
            // min-stem: the segment off each mount before the first/last bend.
            if c.len() >= 3 {
                let first = seg_len(&c[0], &c[1]);
                let last = seg_len(&c[c.len() - 1], &c[c.len() - 2]);
                if first + EPS < MIN_SURFACE_STEM || last + EPS < MIN_SURFACE_STEM {
                    a.short_stems.push(i);
                }
            }
        }
        // flush-to-surface: ANY segment running flush along a node face is a min-stem
        // violation — the verboten "L with a side flush to a surface" (a grazing stem
        // that clings to the wall instead of escaping perpendicular). The length check
        // above misses it (a flush stem can be long); this catches it geometrically.
        let flushed = rects
            .iter()
            .any(|rect| r.windows(2).any(|w| segment_flush_to_rect(&w[0], &w[1], rect)));
        if flushed && !a.short_stems.contains(&i) {
            a.short_stems.push(i);
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
        let a = audit_routes(&[dogleg], &[]);
        assert_eq!(a.doglegs, vec![0], "dogleg must be flagged");
        assert!(!a.is_clean());
    }

    #[test]
    fn accepts_a_stemmed_jog() {
        // A 2-bend facing jog whose end segments are both proper stems (20 ≥
        // MIN_SURFACE_STEM=16) is the legal §0 stem exception — stems are the sole
        // exception to the Z rule, so this is NOT a forbidden Z.
        let jog = vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(40.0, 10.0)];
        let a = audit_routes(&[jog], &[]);
        assert!(a.staircases.is_empty(), "stemmed jog is legal, not a Z");
        assert_eq!(a.cees, 1, "counted as a legal 2-bend shape");
        assert!(a.is_clean());
    }

    #[test]
    fn flags_a_short_stem_jog() {
        // The same jog but with a first stem of only 8 < MIN_SURFACE_STEM: the stem
        // exception does NOT apply to a too-short stem, so it stays a forbidden Z.
        let jog = vec![p(0.0, 0.0), p(8.0, 0.0), p(8.0, 10.0), p(40.0, 10.0)];
        let a = audit_routes(&[jog], &[]);
        assert_eq!(a.staircases, vec![0], "short-stem jog stays forbidden");
        assert!(!a.is_clean());
    }

    #[test]
    fn flags_a_staircase() {
        // A 3-bend staircase (≥5 corners) is forbidden regardless of stem length —
        // the exception is for the single stemmed jog, not multi-bend staircases.
        let s = vec![
            p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(40.0, 10.0), p(40.0, 20.0),
        ];
        let a = audit_routes(&[s], &[]);
        assert_eq!(a.staircases, vec![0], "≥3-bend staircase stays forbidden");
        assert!(!a.is_clean());
    }

    #[test]
    fn flags_an_l_flush_to_a_surface() {
        // An L whose first side drops straight DOWN the source node's right face
        // (x=50) instead of escaping perpendicular — a side flush to a surface, the
        // verboten grazing L. The length check would miss it (the side is 75px long).
        let rect = Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 };
        let l = vec![p(50.0, 25.0), p(50.0, 100.0), p(120.0, 100.0)];
        let a = audit_routes(&[l], &[rect]);
        assert_eq!(a.short_stems, vec![0], "L flush to a surface must be flagged");
        assert!(!a.is_clean());
    }

    #[test]
    fn accepts_a_perpendicular_stem_off_a_surface() {
        // Same endpoints, but the route escapes the right face perpendicular first:
        // it only TOUCHES the face at the mount, never runs along it.
        let rect = Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 };
        let l = vec![p(50.0, 25.0), p(120.0, 25.0), p(120.0, 100.0)];
        let a = audit_routes(&[l], &[rect]);
        assert!(a.short_stems.is_empty(), "a perpendicular stem is not flush");
        assert!(a.is_clean());
    }

    #[test]
    fn flags_a_non_orthogonal_segment() {
        let diag = vec![p(0.0, 0.0), p(10.0, 10.0)];
        let a = audit_routes(&[diag], &[]);
        assert_eq!(a.non_orthogonal, vec![0], "diagonal segment must be flagged");
    }

    #[test]
    fn flags_an_unrouted_edge() {
        let a = audit_routes(&[Vec::new()], &[]);
        assert_eq!(a.unrouted, vec![0]);
    }

    #[test]
    fn flags_a_short_stem() {
        // L whose first stem is only 5 < MIN_SURFACE_STEM (16) — bends at the wall.
        let l = vec![p(0.0, 0.0), p(0.0, 5.0), p(40.0, 5.0)];
        let a = audit_routes(&[l], &[]);
        assert_eq!(a.short_stems, vec![0], "stem below MIN_SURFACE_STEM must be flagged");
    }

    #[test]
    fn flags_a_channel_overlap() {
        // two vertical routes on x=0 sharing the run y[5,10].
        let r0 = vec![p(0.0, 0.0), p(0.0, 10.0)];
        let r1 = vec![p(0.0, 5.0), p(0.0, 15.0)];
        let a = audit_routes(&[r0, r1], &[]);
        assert_eq!(a.channel_overlaps, vec![(0, 1)], "shared channel must be flagged");
    }

    #[test]
    fn flags_close_parallel_channels_under_an_arrowhead() {
        // two horizontal runs 4px apart, overlapping in x → closer than an arrowhead
        // (no exact overlap, so channel_overlaps stays empty; tight_channels catches it).
        let r0 = vec![p(0.0, 100.0), p(50.0, 100.0)];
        let r1 = vec![p(10.0, 104.0), p(60.0, 104.0)];
        let a = audit_routes(&[r0, r1], &[]);
        assert_eq!(a.channel_overlaps, Vec::<(usize, usize)>::new(), "not an exact overlap");
        assert_eq!(a.tight_channels, vec![(0, 1)], "4px < arrowhead must be flagged");
    }

    #[test]
    fn parallel_runs_an_arrowhead_apart_are_not_flagged() {
        let r0 = vec![p(0.0, 100.0), p(50.0, 100.0)];
        let r1 = vec![p(10.0, 100.0 + MIN_CHANNEL_CLEARANCE + 1.0), p(60.0, 100.0 + MIN_CHANNEL_CLEARANCE + 1.0)];
        let a = audit_routes(&[r0, r1], &[]);
        assert!(a.tight_channels.is_empty(), "≥ arrowhead apart is fine: {a:?}");
    }

    #[test]
    fn clean_routing_passes_the_gate_and_counts_shapes() {
        // straight, L (stems 20≥16), C arch (stems 20≥16) — all legal §0 shapes.
        let straight = vec![p(0.0, 0.0), p(40.0, 0.0)];
        let ell = vec![p(0.0, 100.0), p(0.0, 120.0), p(40.0, 120.0)];
        let cee = vec![p(0.0, 200.0), p(0.0, 180.0), p(40.0, 180.0), p(40.0, 200.0)];
        let a = audit_routes(&[straight, ell, cee], &[]);
        assert!(a.is_clean(), "no forbidden artifacts: {a:?}");
        assert_eq!((a.straight, a.ells, a.cees), (1, 1, 1));
    }
}
