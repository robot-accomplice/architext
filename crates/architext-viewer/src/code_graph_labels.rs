//! Which node labels to draw, and where on screen.
//!
//! Labels are the LEVEL-OF-DETAIL payoff, not an overview feature. The
//! reference graph the maintainer set this against shows NO labels at all when
//! zoomed out — the overview is read entirely from shape — and reveals them as
//! you zoom in. Drawing 3,638 of them at any zoom is illegible at best; drawing
//! them at overview zoom is the thing the reference deliberately does not do.
//!
//! Pure: no DOM, no WebGL, no Leptos. The view renders whatever this returns as
//! positioned text, so the decision of WHAT is readable stays unit-testable
//! without a browser — the same split `code_graph_graph` uses for culling.

/// Below this camera zoom, no labels at all.
///
/// Architext's own function tier frames its whole graph at zoom ~0.63 (measured
/// via the `camera_refit` diagnostics event), so this sits comfortably above
/// "the whole graph is on screen": you have to have zoomed INTO a
/// neighbourhood before text appears.
pub const LABEL_MIN_ZOOM: f32 = 1.5;

/// Hard ceiling on labels drawn at once, however far you zoom in.
///
/// Text is the most expensive thing on the canvas and the first thing to turn a
/// legible neighbourhood into a wall of words. When more nodes qualify than
/// this, the highest-DEGREE ones win: at any zoom the hubs are what orient you,
/// and a leaf's name is discoverable by selecting it.
pub const MAX_LABELS: usize = 60;

/// One label to draw: the node it belongs to and its position in canvas
/// (backing-store) pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedLabel {
    pub node: usize,
    pub x: f32,
    pub y: f32,
}

/// Screen-space camera, matching the renderer's contract exactly
/// (`screen = world * zoom + pan`).
#[derive(Debug, Clone, Copy)]
pub struct LabelCamera {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub width: f32,
    pub height: f32,
}

/// The labels worth drawing for the current camera.
///
/// `visible` is the filter cull mask — a node the user's own filters hide must
/// never be named, or the view would assert the presence of something it is
/// deliberately not drawing.
pub fn placed_labels(
    positions: &[(f32, f32)],
    degree: &[u32],
    visible: &[bool],
    camera: LabelCamera,
    max_labels: usize,
) -> Vec<PlacedLabel> {
    if camera.zoom < LABEL_MIN_ZOOM {
        return Vec::new();
    }
    let mut candidates: Vec<(u32, PlacedLabel)> = positions
        .iter()
        .enumerate()
        .filter(|&(i, _)| visible.get(i).copied().unwrap_or(false))
        .filter_map(|(i, &(wx, wy))| {
            let x = wx * camera.zoom + camera.pan_x;
            let y = wy * camera.zoom + camera.pan_y;
            // On-screen only. A label placed outside the canvas costs the same
            // as one inside it and can never be read.
            (x >= 0.0 && x <= camera.width && y >= 0.0 && y <= camera.height)
                .then(|| (degree.get(i).copied().unwrap_or(0), PlacedLabel { node: i, x, y }))
        })
        .collect();

    if candidates.len() > max_labels {
        // Highest degree first, node index as the tiebreak so the same camera
        // always yields the same labels — a set that reshuffled between frames
        // would flicker.
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.node.cmp(&b.1.node)));
        candidates.truncate(max_labels);
    }
    candidates.into_iter().map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(zoom: f32) -> LabelCamera {
        LabelCamera { pan_x: 0.0, pan_y: 0.0, zoom, width: 1000.0, height: 1000.0 }
    }

    #[test]
    fn the_overview_carries_no_labels_at_all() {
        // WHY: the reference this is measured against shows none when zoomed
        // out — the overview is read from SHAPE, and text at that scale is the
        // wall of words it deliberately avoids. This is the behaviour, not a
        // performance shortcut, so it is asserted rather than tuned.
        let pos = vec![(10.0, 10.0), (20.0, 20.0)];
        let deg = vec![5, 5];
        let vis = vec![true, true];
        assert!(placed_labels(&pos, &deg, &vis, camera(LABEL_MIN_ZOOM - 0.01), MAX_LABELS).is_empty());
        assert_eq!(placed_labels(&pos, &deg, &vis, camera(LABEL_MIN_ZOOM), MAX_LABELS).len(), 2);
    }

    #[test]
    fn a_node_off_screen_is_never_labelled() {
        // WHY: at label zoom most of a 3,638-node graph is outside the canvas.
        // Placing labels for all of it would cost the same as placing the
        // readable ones and produce nothing a user can see.
        let pos = vec![(10.0, 10.0), (10_000.0, 10.0), (-10_000.0, 10.0)];
        let deg = vec![1, 1, 1];
        let vis = vec![true, true, true];
        let out = placed_labels(&pos, &deg, &vis, camera(2.0), MAX_LABELS);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].node, 0);
    }

    #[test]
    fn a_filtered_out_node_is_never_labelled() {
        // WHY: naming a node the filters are deliberately hiding would assert
        // the presence of something the view is not drawing.
        let pos = vec![(10.0, 10.0), (20.0, 20.0)];
        let deg = vec![9, 9];
        let vis = vec![false, true];
        let out = placed_labels(&pos, &deg, &vis, camera(2.0), MAX_LABELS);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].node, 1);
    }

    #[test]
    fn when_too_many_qualify_the_hubs_win_and_the_choice_is_stable() {
        // WHY: hubs are what orient you in a neighbourhood; a leaf's name is
        // one click away. And the set must not reshuffle between frames — a
        // label that flickers in and out is worse than no label.
        let pos: Vec<(f32, f32)> = (0..10).map(|i| (i as f32, 0.0)).collect();
        let deg: Vec<u32> = (0..10).map(|i| i as u32).collect(); // node 9 is the biggest hub
        let vis = vec![true; 10];
        let out = placed_labels(&pos, &deg, &vis, camera(2.0), 3);
        assert_eq!(out.len(), 3);
        let picked: Vec<usize> = out.iter().map(|l| l.node).collect();
        assert_eq!(picked, vec![9, 8, 7], "the three highest-degree nodes");
        let again = placed_labels(&pos, &deg, &vis, camera(2.0), 3);
        assert_eq!(out, again, "same camera must yield the same labels");
    }

    #[test]
    fn a_label_sits_where_the_renderer_draws_its_node() {
        // WHY: labels are positioned by THIS module and nodes by the vertex
        // shader. If the two transforms disagree the text drifts off its dot,
        // so this pins the same `screen = world * zoom + pan` contract.
        let pos = vec![(100.0, 50.0)];
        let cam = LabelCamera { pan_x: 30.0, pan_y: -5.0, zoom: 2.0, width: 1000.0, height: 1000.0 };
        let out = placed_labels(&pos, &[1], &[true], cam, MAX_LABELS);
        assert_eq!(out[0].x, 100.0 * 2.0 + 30.0);
        assert_eq!(out[0].y, 50.0 * 2.0 - 5.0);
    }
}
