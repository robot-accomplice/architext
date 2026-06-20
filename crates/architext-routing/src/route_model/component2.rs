//! Component 2 — forced monotone detour (ROUTING_DETERMINISTIC_MODEL.md §3).
//!
//! Reached only when Component 1 proves no clean 0/1-bend route clears the
//! obstacle field. It draws the **monotone** path around the obstacles with the
//! fewest bends (then shortest), via a forward-only Dijkstra on the Hanan grid of
//! obstacle edges. Because moves are forward-only (toward B on each axis), the
//! result is monotone by construction — **a dogleg can never arise**, at any
//! depth. Returns `None` only if no monotone clearing path exists at all (a truly
//! boxed-in pocket, which a layered diagram does not produce).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use super::EPS;
use crate::model::{Point, Rect};
use crate::route_geometry::segment_intersects_rect;

/// Dijkstra heap item: `(bends, length_milli, i, j, last_axis)`, min-ordered via
/// `Reverse`. The cost prefix `(bends, length_milli)` drives selection; the grid
/// coords + axis make it a deterministic, total order.
type HeapItem = Reverse<(usize, i64, usize, usize, u8)>;

/// Gutter placed just outside an obstacle edge so a grid line exists to route
/// alongside it. Pixel-scale; only needs to be positive to guarantee clearance.
const DETOUR_GUTTER: f64 = 8.0;

/// Forward-sorted unique coordinates between the two endpoints (inclusive) plus
/// obstacle edges (± gutter) that fall in range. "Forward" = the direction from
/// `from` to `to`, so index 0 is `from`'s coordinate and the last is `to`'s.
fn axis_grid(from: f64, to: f64, edges: impl Iterator<Item = f64>) -> Vec<f64> {
    let lo = from.min(to);
    let hi = from.max(to);
    let mut xs: Vec<f64> = vec![from, to];
    for e in edges {
        if e > lo + EPS && e < hi - EPS {
            xs.push(e);
        }
    }
    // dedup within EPS
    let ascending = to >= from;
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < EPS);
    if !ascending {
        xs.reverse();
    }
    xs
}

/// True if the orthogonal segment crosses any obstacle's interior.
fn blocked(a: &Point, b: &Point, obstacles: &[Rect]) -> bool {
    obstacles
        .iter()
        .any(|r| segment_intersects_rect(a, b, r, 0.0))
}

/// Min-bend monotone path from `p_a` to `p_b` clearing `obstacles` (already
/// excluding the endpoint nodes). `None` if no monotone clearing path exists.
pub fn monotone_detour(p_a: &Point, p_b: &Point, obstacles: &[Rect]) -> Option<Vec<Point>> {
    let xs = axis_grid(
        p_a.x,
        p_b.x,
        obstacles
            .iter()
            .flat_map(|r| [r.x - DETOUR_GUTTER, r.x + r.width + DETOUR_GUTTER]),
    );
    let ys = axis_grid(
        p_a.y,
        p_b.y,
        obstacles
            .iter()
            .flat_map(|r| [r.y - DETOUR_GUTTER, r.y + r.height + DETOUR_GUTTER]),
    );
    let nx = xs.len();
    let ny = ys.len();
    let pt = |i: usize, j: usize| Point { x: xs[i], y: ys[j] };
    let goal = (nx - 1, ny - 1);

    // State = (grid node, last-move axis: 0 none, 1 x, 2 y). Cost = (bends, len).
    // Dijkstra with a min-heap keyed by Reverse((bends, len_milli, ...)).
    // dist: (i,j,axis) -> (bends, len_milli). came: -> prev (i,j,axis).
    let key = |i: usize, j: usize, ax: u8| (i * ny + j) * 3 + ax as usize;
    let mut dist: HashMap<usize, (usize, i64)> = HashMap::new();
    let mut came: HashMap<usize, (usize, usize, u8)> = HashMap::new();
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();

    dist.insert(key(0, 0, 0), (0, 0));
    heap.push(Reverse((0, 0, 0, 0, 0))); // (bends, len_milli, i, j, axis)

    while let Some(Reverse((bends, len_milli, i, j, ax))) = heap.pop() {
        if (i, j) == goal {
            // reconstruct
            let mut path = vec![pt(i, j)];
            let mut cur = (i, j, ax);
            while let Some(&(pi, pj, pax)) = came.get(&key(cur.0, cur.1, cur.2)) {
                path.push(pt(pi, pj));
                cur = (pi, pj, pax);
            }
            path.reverse();
            return Some(simplify(path));
        }
        if dist.get(&key(i, j, ax)).map(|&(b, l)| (bends, len_milli) > (b, l)).unwrap_or(false) {
            continue; // stale
        }
        // forward neighbours: +x (i+1) and +y (j+1)
        let relax = |ni: usize, nj: usize, nax: u8, seg_len: f64, heap: &mut BinaryHeap<_>, dist: &mut HashMap<usize,(usize,i64)>, came: &mut HashMap<usize,(usize,usize,u8)>| {
            let a = pt(i, j);
            let b = pt(ni, nj);
            if blocked(&a, &b, obstacles) {
                return;
            }
            let added_bend = if ax != 0 && ax != nax { 1 } else { 0 };
            let nb = bends + added_bend;
            let nl = len_milli + (seg_len * 1000.0).round() as i64;
            let k = key(ni, nj, nax);
            if dist.get(&k).map(|&(b0, l0)| (nb, nl) < (b0, l0)).unwrap_or(true) {
                dist.insert(k, (nb, nl));
                came.insert(k, (i, j, ax));
                heap.push(Reverse((nb, nl, ni, nj, nax)));
            }
        };
        if i + 1 < nx {
            let len = (xs[i + 1] - xs[i]).abs();
            relax(i + 1, j, 1, len, &mut heap, &mut dist, &mut came);
        }
        if j + 1 < ny {
            let len = (ys[j + 1] - ys[j]).abs();
            relax(i, j + 1, 2, len, &mut heap, &mut dist, &mut came);
        }
    }
    None
}

/// Drop collinear interior vertices so the path is a reduced orthogonal polyline.
fn simplify(points: Vec<Point>) -> Vec<Point> {
    if points.len() < 3 {
        return points;
    }
    let mut out: Vec<Point> = vec![points[0].clone()];
    for i in 1..points.len() - 1 {
        let a = out[out.len() - 1].clone();
        let b = &points[i];
        let c = &points[i + 1];
        // keep b only if the direction changes at b
        let abx = (b.x - a.x).abs() > EPS;
        let bcx = (c.x - b.x).abs() > EPS;
        if abx != bcx {
            out.push(b.clone());
        }
    }
    out.push(points[points.len() - 1].clone());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_model::{bends, monotone};

    fn p(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn no_obstacle_is_a_clean_l() {
        // offset endpoints, free space ⇒ min-bend monotone path is a single L.
        let r = monotone_detour(&p(0.0, 0.0), &p(100.0, 80.0), &[]).unwrap();
        assert!(monotone(&r));
        assert_eq!(bends(&r), 1);
    }

    #[test]
    fn detours_around_a_blocking_box_monotonically() {
        // Offset mounts (0,0)→(120,60). Two boxes block BOTH clean L's (the y=0
        // and the y=60 horizontals through the middle), forcing a ≥2-bend
        // monotone staircase that threads between them. Never doubles back.
        let obs = vec![
            Rect { x: 40.0, y: -10.0, width: 40.0, height: 20.0 }, // covers y∈[-10,10]
            Rect { x: 40.0, y: 50.0, width: 40.0, height: 20.0 },  // covers y∈[50,70]
        ];
        let r = monotone_detour(&p(0.0, 0.0), &p(120.0, 60.0), &obs).unwrap();
        assert!(monotone(&r), "detour must never double back");
        assert!(bends(&r) >= 2, "both Ls blocked ⇒ >=2 bends");
        for w in r.windows(2) {
            assert!(!blocked(&w[0], &w[1], &obs));
        }
    }

    #[test]
    fn collinear_blocked_mounts_have_no_monotone_detour() {
        // Equal endpoint-y with a centreline box: any way around requires a
        // y-reversal, which is a dogleg. Monotone ⇒ None. The caller must pick
        // different surfaces (eviction), not accept a dogleg.
        let box_ = Rect { x: 40.0, y: -5.0, width: 20.0, height: 30.0 };
        assert!(monotone_detour(&p(0.0, 0.0), &p(100.0, 0.0), &[box_]).is_none());
    }

    #[test]
    fn result_is_always_monotone_never_a_dogleg() {
        // a few obstacle configurations — the invariant holds for all.
        let cfgs = [
            vec![Rect { x: 30.0, y: -50.0, width: 10.0, height: 60.0 }],
            vec![Rect { x: 30.0, y: 10.0, width: 10.0, height: 60.0 }],
            vec![
                Rect { x: 30.0, y: -50.0, width: 10.0, height: 70.0 },
                Rect { x: 70.0, y: 0.0, width: 10.0, height: 70.0 },
            ],
        ];
        for obs in cfgs {
            if let Some(r) = monotone_detour(&p(0.0, 0.0), &p(120.0, 40.0), &obs) {
                assert!(monotone(&r));
            }
        }
    }
}
