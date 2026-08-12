//! ER (entity-relationship) layout — a sibling of `plan_diagram`, not a
//! variant of it.
//!
//! `plan_diagram` sizes every node from `diagram_config` (136x54 for all of
//! them). An ER box cannot work that way: a three-attribute entity and a
//! fifteen-attribute entity must not share a height. That single difference is
//! why this is its own engine rather than a flag on the existing one, and
//! `plan_diagram` is not modified by this module.
//!
//! What it does share is the crate's discipline: deterministic (same input
//! yields the same plan, byte for byte), no wall-clock, no randomness, and one
//! source compiled both native (serve) and to WASM (viewer).
//!
//! ## Why there are no columns
//!
//! An earlier version laid entities out in layered columns, the way flows and
//! C4 are laid out. That was wrong for this data. Layering encodes DIRECTION,
//! and it earns its place when a graph is directed and roughly acyclic -- a
//! call graph, a flow. ER relationships are an undirected graph with cycles, so
//! the columns carried no label, encoded no category, and could not be read for
//! anything. They were pure constraint, paid for in a tall canvas, a routing
//! channel every edge had to share, and ports crammed onto one face of a box.
//!
//! Placement is free 2D instead: related entities pull together, all of them
//! push apart, and overlapping boxes separate. Proximity carries the structure,
//! which is what a reader can actually use.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::model::Point;
use crate::route_constants::MOUNT_COST;

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------
// Central, named, and used once each. Text width is estimated rather than
// measured because layout runs in WASM with no font metrics available and must
// produce identical geometry natively; a measured width would make the plan
// depend on the host's font stack and break determinism.

/// Height of the entity-name header band.
pub const HEADER_H: f64 = 26.0;
/// Height of one attribute row.
pub const ROW_H: f64 = 17.0;
/// Vertical padding inside the box, above and below the attribute rows.
pub const BOX_PAD_Y: f64 = 5.0;
/// Nominal advance width of one character at the box's font size. Published
/// so the renderer can truncate a row to the width the layout gave it.
pub const CHAR_W: f64 = 6.6;
/// Horizontal padding inside the box, left and right.
pub const BOX_PAD_X: f64 = 12.0;
const BOX_MIN_W: f64 = 132.0;
// 320 clipped a real row on the first real schema Architext modelled
// ("target_release_id  slug  -> release_summary" needs 321.2px). Snake_case
// column names next to snake_case entity names are the normal case, not the
// pathological one, so the cap has headroom for them now.
const BOX_MAX_W: f64 = 380.0;

/// How much wider than tall the grid aims to be. A diagram is read on a screen
/// that is wider than tall, so the grid matches that rather than being square.
const GRID_ASPECT: f64 = 1.6;
/// Clear space between grid columns and rows. Uniform, which is what makes the
/// spacing balanced everywhere instead of dense in the middle.
const GRID_GAP_X: f64 = 96.0;
const GRID_GAP_Y: f64 = 64.0;
/// What a crossing costs the arrangement, in the same units as edge length.
/// High enough that trading a crossing for a longer edge is worthwhile, which
/// is the trade a reader wants.
const CROSSING_PENALTY: f64 = 12.0;
/// Passes of pairwise cell swapping. Bounded for predictable layout time; the
/// loop also exits as soon as a pass finds no strict improvement.
const ARRANGEMENT_PASSES: usize = 6;

/// Every face a line may attach to, in a fixed order so scoring ties resolve
/// the same way on every run.
const SIDES: [Side; 4] = [Side::Right, Side::Left, Side::Bottom, Side::Top];
/// Passes of surface re-selection. Each edge re-picks against the others'
/// current choices; the loop exits early once a pass changes nothing.
const SURFACE_PASSES: usize = 3;
/// Widest half-width a relationship label is assumed to occupy when measuring
/// the canvas. Generous on purpose: under-measuring clips a label off the edge,
/// and the cost of over-measuring is a little whitespace.
const LABEL_MAX_HALF_W: f64 = 60.0;
/// Half-height of a relationship label, for the clearance test.
const LABEL_HALF_H: f64 = 9.0;
/// How many positions either side of the middle to try when a label lands on a
/// box. Bounded so layout time stays predictable.
const LABEL_SAMPLES: usize = 12;

/// How far a self-relationship's loop stands off the box it belongs to.
///
/// A relationship from an entity to ITSELF has both ends on one box. Treated
/// like any other edge it ran from the right face to the left face -- straight
/// through the entity, with its label stranded inside the box and unreadable.
/// It needs to leave the box and come back, visibly.
const SELF_LOOP_EXT: f64 = 38.0;

/// Clearance a path keeps from a box it is routing around.
const DETOUR_CLEARANCE: f64 = 18.0;

/// How far in from a box's top and bottom corners the outermost edge port sits,
/// so a line never appears to attach to the corner itself.
const PORT_INSET: f64 = 10.0;
/// Canvas margin on every side.
const MARGIN: f64 = 28.0;

// ---------------------------------------------------------------------------
// Input — mirrors entities.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErInput {
    #[serde(default)]
    pub entities: Vec<ErEntityInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErEntityInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub owner_node_id: Option<String>,
    #[serde(default)]
    pub data_class_ids: Vec<String>,
    #[serde(default)]
    pub attributes: Vec<ErAttributeInput>,
    #[serde(default)]
    pub relationships: Vec<ErRelationshipInput>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErAttributeInput {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub references: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErRelationshipInput {
    pub to: String,
    pub cardinality: String,
    #[serde(default)]
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Which edge of a box a line attaches to.
///
/// All four, because with free placement a neighbour can be in any direction.
/// While entities sat in columns only Left and Right were reachable, which is
/// why every line used to leave sideways even when its target was directly
/// above or below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Right,
    Left,
    Bottom,
    Top,
}

impl Side {
    fn opposite(self) -> Side {
        match self {
            Side::Right => Side::Left,
            Side::Left => Side::Right,
            Side::Bottom => Side::Top,
            Side::Top => Side::Bottom,
        }
    }
    /// Whether ports on this side are spread horizontally (top/bottom) rather
    /// than vertically (left/right).
    fn is_horizontal_face(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

/// Which crow's-foot glyph terminates one end of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErFoot {
    One,
    Many,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErRow {
    pub name: String,
    pub type_name: String,
    pub key: Option<String>,
    pub required: bool,
    /// The entity this attribute's foreign key names, when it names one.
    pub references: Option<String>,
    /// False when `references` is set but no relationship declares that edge.
    ///
    /// `relationships` is the sole source of rendered edges, so such a foreign
    /// key draws nothing. That is legitimate -- an author may not want the edge
    /// -- but it is invisible on the diagram, so the viewer annotates the row
    /// instead of leaving the reader to wonder where the line went.
    pub relationship_declared: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErBox {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rows: Vec<ErRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub cardinality: String,
    pub points: Vec<Point>,
    pub from_foot: ErFoot,
    pub to_foot: ErFoot,
    pub label_x: f64,
    pub label_y: f64,
    /// Points where this edge passes OVER one drawn earlier, in order along the
    /// line. The renderer draws a small arc at each so a crossing cannot be
    /// mistaken for a join.
    pub hops: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErPlan {
    pub boxes: Vec<ErBox>,
    pub edges: Vec<ErEdge>,
    pub canvas_width: f64,
    pub canvas_height: f64,
}

// ---------------------------------------------------------------------------
// Sizing
// ---------------------------------------------------------------------------

/// Box height is a function of attribute count. This is the reason ER needs its
/// own engine.
fn box_height(attribute_count: usize) -> f64 {
    HEADER_H + BOX_PAD_Y * 2.0 + attribute_count as f64 * ROW_H
}

/// Marker the renderer appends to a foreign key that draws no edge.
///
/// Published so the width estimate sizes for the text that is ACTUALLY drawn.
/// It was not, and the annotated row overflowed its box by 153px into the
/// neighbouring entity -- visible immediately on screen and invisible to every
/// unit test, because nothing in Rust knew what the viewer appended.
///
/// Kept short deliberately. The full sentence is a tooltip; inlining it would
/// add 27 characters to a row, and a schema where many foreign keys go
/// undeclared would size every box to fit a sentence.
pub const UNDECLARED_MARKER: &str = " (not drawn)";

/// X offset from the box's left edge to the key glyph column.
pub const KEY_X: f64 = 10.0;
/// X offset from the box's left edge to the attribute text.
pub const ROW_TEXT_X: f64 = 32.0;

/// Characters the renderer spends joining a row to its reference clause:
/// two spaces, the arrow, and a space ("  \u{2192} ").
///
/// Named because it was counted as 3 while the renderer drew 4, which cost one
/// character of width and clipped the marker to "(not dra\u{2026}". An estimate
/// that is even one character short truncates the very text it is estimating.
const REF_SEPARATOR_CHARS: usize = 4;

/// Estimated character count of an attribute row's text: `name  type` plus the
/// reference clause when there is one.
fn row_text_len(attr: &ErAttributeInput, declared: bool) -> usize {
    let ref_len = match attr.references.as_deref() {
        None => 0,
        Some(r) => {
            r.chars().count()
                + REF_SEPARATOR_CHARS
                + if declared { 0 } else { UNDECLARED_MARKER.chars().count() }
        }
    };
    attr.name.chars().count() + 2 + attr.type_name.chars().count() + ref_len
}

/// `is_declared` answers whether a foreign key's target is joined by a
/// relationship, which decides whether the row carries the marker and so how
/// wide the box has to be.
fn box_width(entity: &ErEntityInput, is_declared: &impl Fn(&str) -> bool) -> f64 {
    let widest_row = entity
        .attributes
        .iter()
        .map(|a| {
            let declared = a.references.as_deref().is_none_or(is_declared);
            ROW_TEXT_X + row_text_len(a, declared) as f64 * CHAR_W
        })
        .fold(0.0_f64, f64::max);
    let header = KEY_X + entity.name.chars().count() as f64 * CHAR_W;
    (widest_row.max(header) + BOX_PAD_X).clamp(BOX_MIN_W, BOX_MAX_W)
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Lay out an ER diagram.
///
/// Deterministic: every ordering decision is resolved by an explicit tie-break
/// on entity id, and no map is iterated in hash order.
pub fn plan_er(input: &ErInput) -> ErPlan {
    let entities = &input.entities;
    let n = entities.len();
    if n == 0 {
        return ErPlan {
            boxes: Vec::new(),
            edges: Vec::new(),
            canvas_width: MARGIN * 2.0,
            canvas_height: MARGIN * 2.0,
        };
    }

    // id -> index. BTreeMap, not HashMap: iteration order must not vary.
    let index_of: BTreeMap<&str, usize> =
        entities.iter().enumerate().map(|(i, e)| (e.id.as_str(), i)).collect();

    // --- adjacency (undirected, for placement only) --------------------------
    let mut adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (i, entity) in entities.iter().enumerate() {
        for rel in &entity.relationships {
            if let Some(&j) = index_of.get(rel.to.as_str()) {
                if i != j {
                    adj[i].insert(j);
                    adj[j].insert(i);
                }
            }
        }
    }

    // Which entity pairs an edge actually connects, in EITHER direction.
    //
    // An attribute's annotation answers "does this foreign key draw an edge?",
    // and an edge exists if either end declared it. Checking only the entity's
    // own `relationships` marks every child's foreign key as undeclared when
    // the parent is the one that declared the link -- which is the normal
    // shape, so nearly every foreign key in a real schema would be flagged
    // while its edge was plainly visible on the diagram.
    let mut related_pairs: BTreeSet<(&str, &str)> = BTreeSet::new();
    for entity in entities {
        for rel in &entity.relationships {
            if index_of.contains_key(rel.to.as_str()) {
                related_pairs.insert((entity.id.as_str(), rel.to.as_str()));
                related_pairs.insert((rel.to.as_str(), entity.id.as_str()));
            }
        }
    }

    // Box sizes first: the grid's column widths and row heights are derived
    // from them, and declared-ness decides whether a row carries the undeclared
    // marker and so how wide its box has to be.
    let widths: Vec<f64> = entities
        .iter()
        .map(|e| {
            let eid = e.id.as_str();
            box_width(e, &|target: &str| related_pairs.contains(&(eid, target)))
        })
        .collect();
    let heights: Vec<f64> = entities.iter().map(|e| box_height(e.attributes.len())).collect();

    // --- placement: a balanced grid, ordered for legibility -----------------
    //
    // Not a force simulation. Physics optimises ENERGY, and a settled energy
    // minimum is a dense core with a sparse rim -- which is what a hub always
    // produces and what made this diagram read as bunched however the constants
    // were tuned. Legibility is a different objective, so it is optimised
    // directly.
    //
    // A grid gives even spacing by CONSTRUCTION: every entity sits in a cell,
    // so no region is ever crowded or empty, and a reader can scan rows and
    // columns instead of hunting a cloud. What is optimised is which entity
    // goes in which cell -- ordered so related entities land near each other
    // and edges stay short.
    //
    // Column count is derived from the BOX shape, not just the entity count.
    // Entity boxes are far wider than tall (roughly 300x120), so picking
    // columns from sqrt(n) alone produced a 2.94:1 strip. Solving
    // cols^2 = aspect * n * avg_height / avg_width gives a grid whose RENDERED
    // proportions land near the target instead of its cell counts.
    let avg_w = widths.iter().sum::<f64>() / n as f64;
    let avg_h = heights.iter().sum::<f64>() / n as f64;
    let n_cols = ((GRID_ASPECT * n as f64 * avg_h / avg_w).sqrt().round() as usize)
        .clamp(1, n);

    // Seed order: breadth-first from the highest-degree entity, so a hub and
    // its neighbours enter the grid consecutively and start out adjacent.
    // Entities with no relationships come last -- nothing places them, so they
    // fill the trailing cells rather than displacing anything that has
    // structure to show.
    let mut slot: Vec<usize> = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut remaining: BTreeSet<usize> = (0..n).filter(|&i| !adj[i].is_empty()).collect();
    while let Some(&seed) = remaining
        .iter()
        .max_by_key(|&&i| (adj[i].len(), std::cmp::Reverse(entities[i].id.as_str())))
    {
        let mut queue = VecDeque::new();
        queue.push_back(seed);
        seen[seed] = true;
        remaining.remove(&seed);
        while let Some(i) = queue.pop_front() {
            slot.push(i);
            for &j in &adj[i] {
                if !seen[j] {
                    seen[j] = true;
                    remaining.remove(&j);
                    queue.push_back(j);
                }
            }
        }
    }
    for (i, placed) in seen.iter().enumerate() {
        if !placed {
            slot.push(i);
        }
    }

    // Undirected edge list, used to score an arrangement.
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, entity) in entities.iter().enumerate() {
        for rel in &entity.relationships {
            if let Some(&j) = index_of.get(rel.to.as_str()) {
                if i != j {
                    pairs.push((i.min(j), i.max(j)));
                }
            }
        }
    }
    pairs.sort_unstable();
    pairs.dedup();

    // Cost of an arrangement: total edge length in cell space, with a mild
    // superlinear term so one very long edge is worse than two short ones --
    // a single line crossing the whole diagram is what costs a reader most.
    let arrangement_cost = |cell_of: &[usize]| -> f64 {
        let cell_xy = |c: usize| ((c % n_cols) as f64, (c / n_cols) as f64);
        let mut cost: f64 = pairs
            .iter()
            .map(|&(i, j)| {
                let ((xi, yi), (xj, yj)) = (cell_xy(cell_of[i]), cell_xy(cell_of[j]));
                let d = ((xi - xj).powi(2) + (yi - yj).powi(2)).sqrt();
                d * d.sqrt()
            })
            .sum();

        // Crossings, estimated in cell space. Length alone is a decent proxy
        // but it is not the same objective: two short edges can still cross,
        // and a crossing costs a reader more than a little extra length. Scored
        // here so the arrangement is chosen for legibility rather than only for
        // compactness.
        for a in 0..pairs.len() {
            for b in (a + 1)..pairs.len() {
                let (p1, p2) = (pairs[a], pairs[b]);
                if p1.0 == p2.0 || p1.0 == p2.1 || p1.1 == p2.0 || p1.1 == p2.1 {
                    continue; // shares an entity: they meet at a box, not in space
                }
                let (a1, a2) = (cell_xy(cell_of[p1.0]), cell_xy(cell_of[p1.1]));
                let (b1, b2) = (cell_xy(cell_of[p2.0]), cell_xy(cell_of[p2.1]));
                if segments_cross_xy(a1, a2, b1, b2) {
                    cost += CROSSING_PENALTY;
                }
            }
        }
        cost
    };

    let mut cell_of: Vec<usize> = vec![0; n];
    for (cell, &e) in slot.iter().enumerate() {
        cell_of[e] = cell;
    }

    // Improve by swapping pairs of cells, keeping only strict improvements so
    // the pass cannot oscillate. Deterministic: fixed order, fixed pass count.
    let mut best = arrangement_cost(&cell_of);
    for _ in 0..ARRANGEMENT_PASSES {
        let mut improved = false;
        for a in 0..n {
            for b in (a + 1)..n {
                let (ea, eb) = (slot[a], slot[b]);
                cell_of[ea] = b;
                cell_of[eb] = a;
                let c = arrangement_cost(&cell_of);
                if c < best - 1e-9 {
                    best = c;
                    slot.swap(a, b);
                    improved = true;
                } else {
                    cell_of[ea] = a;
                    cell_of[eb] = b;
                }
            }
        }
        if !improved {
            break;
        }
    }

    // --- cell geometry -------------------------------------------------------
    // Column widths and row heights come from the widest and tallest box in
    // each, so a 15-attribute entity never overlaps its neighbour and the grid
    // still reads as aligned.
    let n_rows = n.div_ceil(n_cols);
    let mut col_w = vec![0.0_f64; n_cols];
    let mut row_h = vec![0.0_f64; n_rows];
    for (cell, &e) in slot.iter().enumerate() {
        let (c, r) = (cell % n_cols, cell / n_cols);
        col_w[c] = col_w[c].max(widths[e]);
        row_h[r] = row_h[r].max(heights[e]);
    }

    let mut col_x = Vec::with_capacity(n_cols);
    let mut acc = 0.0;
    for w in &col_w {
        col_x.push(acc);
        acc += w + GRID_GAP_X;
    }
    let mut row_y = Vec::with_capacity(n_rows);
    acc = 0.0;
    for h in &row_h {
        row_y.push(acc);
        acc += h + GRID_GAP_Y;
    }

    let mut px = vec![0.0_f64; n];
    let mut py = vec![0.0_f64; n];
    for (cell, &e) in slot.iter().enumerate() {
        let (c, r) = (cell % n_cols, cell / n_cols);
        // Centred in its cell, so uneven box sizes still read as a grid.
        px[e] = col_x[c] + (col_w[c] - widths[e]) / 2.0 + widths[e] / 2.0;
        py[e] = row_y[r] + (row_h[r] - heights[e]) / 2.0 + heights[e] / 2.0;
    }

    // --- geometry ------------------------------------------------------------
    // Shift into positive space and round, so the plan is stable to read and
    // diff rather than carrying float noise.
    let min_x = px
        .iter()
        .zip(&widths)
        .map(|(x, w)| x - w / 2.0)
        .fold(f64::INFINITY, f64::min);
    let min_y = py
        .iter()
        .zip(&heights)
        .map(|(y, h)| y - h / 2.0)
        .fold(f64::INFINITY, f64::min);

    let mut boxes: Vec<ErBox> = Vec::with_capacity(n);
    for (i, entity) in entities.iter().enumerate() {
        boxes.push(ErBox {
            id: entity.id.clone(),
            name: entity.name.clone(),
            x: ((px[i] - widths[i] / 2.0 - min_x + MARGIN) * 10.0).round() / 10.0,
            y: ((py[i] - heights[i] / 2.0 - min_y + MARGIN) * 10.0).round() / 10.0,
            width: widths[i],
            height: heights[i],
            rows: entity
                .attributes
                .iter()
                .map(|a| ErRow {
                    name: a.name.clone(),
                    type_name: a.type_name.clone(),
                    key: a.key.clone(),
                    required: a.required,
                    references: a.references.clone(),
                    relationship_declared: a.references.as_deref().is_none_or(|t| {
                        related_pairs.contains(&(entity.id.as_str(), t))
                    }),
                })
                .collect(),
        });
    }

    let mut edges = route_edges(&boxes, input);

    // The canvas has to cover the EDGES too, not just the boxes.
    //
    // Routes leave the boxes deliberately: a self-relationship loops 38px above
    // and right of its entity, and a gutter detour runs outside the outer rows.
    // Sizing the canvas from box extents alone put anything on the top or left
    // row into NEGATIVE coordinates -- outside the viewBox, clipped, and
    // unreachable by scrolling, because there is nothing to scroll to.
    //
    // So the whole plan is measured, shifted into positive space, and the
    // canvas takes the union. Nothing can be laid out where it cannot be seen.
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for b in &boxes {
        min_x = min_x.min(b.x);
        min_y = min_y.min(b.y);
        max_x = max_x.max(b.x + b.width);
        max_y = max_y.max(b.y + b.height);
    }
    for e in &edges {
        for p in e.points.iter().chain(e.hops.iter()) {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        // Labels are drawn centred on their anchor, so they extend either side.
        min_x = min_x.min(e.label_x - LABEL_MAX_HALF_W);
        max_x = max_x.max(e.label_x + LABEL_MAX_HALF_W);
        min_y = min_y.min(e.label_y - LABEL_HALF_H);
        max_y = max_y.max(e.label_y + LABEL_HALF_H);
    }

    let (dx, dy) = (MARGIN - min_x, MARGIN - min_y);
    if dx.abs() > f64::EPSILON || dy.abs() > f64::EPSILON {
        for b in &mut boxes {
            b.x += dx;
            b.y += dy;
        }
        for e in &mut edges {
            for p in e.points.iter_mut().chain(e.hops.iter_mut()) {
                p.x += dx;
                p.y += dy;
            }
            e.label_x += dx;
            e.label_y += dy;
        }
    }

    let canvas_width = max_x - min_x + MARGIN * 2.0;
    let canvas_height = max_y - min_y + MARGIN * 2.0;

    ErPlan { boxes, edges, canvas_width, canvas_height }
}

/// Crow's-foot glyphs for a cardinality. An unrecognised value cannot reach
/// here through validated data (`cardinality` is an enumerated field), so it
/// degrades to one-to-one rather than failing the render.
fn feet(cardinality: &str) -> (ErFoot, ErFoot) {
    match cardinality {
        "one-to-many" => (ErFoot::One, ErFoot::Many),
        "many-to-many" => (ErFoot::Many, ErFoot::Many),
        _ => (ErFoot::One, ErFoot::One),
    }
}

/// Where a relationship's label sits on its path.
///
/// The midpoint is the obvious choice and often the wrong one: on Architext's
/// own model six of sixteen labels landed on top of an entity. Painting labels
/// last made them readable, but a label lying across a box is still noise. So
/// the path is sampled and the position closest to the middle that is CLEAR of
/// every box wins; if the whole path is covered, the midpoint stands, because
/// somewhere is better than nowhere.
fn label_anchor(path: &[Point], boxes: &[ErBox], half_w: f64) -> (f64, f64) {
    let point_at = |t: f64| -> (f64, f64) {
        let total: f64 = path
            .windows(2)
            .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
            .sum();
        let mut want = total * t;
        for w in path.windows(2) {
            let seg = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
            if seg >= want {
                let f = if seg > 0.0 { want / seg } else { 0.0 };
                return (w[0].x + (w[1].x - w[0].x) * f, w[0].y + (w[1].y - w[0].y) * f);
            }
            want -= seg;
        }
        let last = path.last().unwrap();
        (last.x, last.y)
    };

    let clear = |x: f64, y: f64| -> bool {
        !boxes.iter().any(|b| {
            x + half_w > b.x
                && x - half_w < b.x + b.width
                && y + LABEL_HALF_H > b.y
                && y - LABEL_HALF_H < b.y + b.height
        })
    };

    // Walk outward from the middle so the label stays as central as it can.
    for step in 0..=LABEL_SAMPLES {
        let delta = step as f64 / LABEL_SAMPLES as f64 * 0.42;
        for t in [0.5 - delta, 0.5 + delta] {
            if !(0.05..=0.95).contains(&t) {
                continue;
            }
            let (x, y) = point_at(t);
            if clear(x, y) {
                return (x, y);
            }
        }
    }
    point_at(0.5)
}

/// The loop drawn for a relationship from an entity to itself.
///
/// Out of the right face, up and over the top-right corner, and back down into
/// the top face. Entirely outside the box, so both the line and its label are
/// legible; the label lands on the outer corner because that is the midpoint of
/// this path.
fn self_loop_path(b: &ErBox) -> Vec<Point> {
    let out_x = b.x + b.width + SELF_LOOP_EXT;
    let up_y = b.y - SELF_LOOP_EXT;
    vec![
        Point { x: b.x + b.width, y: b.y + b.height * 0.35 },
        Point { x: out_x, y: b.y + b.height * 0.35 },
        Point { x: out_x, y: up_y },
        Point { x: b.x + b.width * 0.6, y: up_y },
        Point { x: b.x + b.width * 0.6, y: b.y },
    ]
}

/// The path shapes worth considering between two ports on two given faces.
///
/// Straight first, because it is shortest and bend-free and wins whenever it is
/// clear. The orthogonal variants exist so the router has something to buy with
/// the bend cost when straight would cut through a box.
fn candidate_shapes(
    a: &Point,
    b: &Point,
    sa: Side,
    sb: Side,
    ba: &ErBox,
    bb: &ErBox,
) -> Vec<Vec<Point>> {
    let mut out = vec![vec![a.clone(), b.clone()]];

    // Two L shapes: turn on one axis first, then the other.
    out.push(vec![a.clone(), Point { x: b.x, y: a.y }, b.clone()]);
    out.push(vec![a.clone(), Point { x: a.x, y: b.y }, b.clone()]);

    // A Z that splits the gap, which is what leaves a face cleanly when both
    // ends use opposing faces.
    if sa.is_horizontal_face() == sb.is_horizontal_face() {
        if sa.is_horizontal_face() {
            let mid = (a.y + b.y) / 2.0;
            out.push(vec![
                a.clone(),
                Point { x: a.x, y: mid },
                Point { x: b.x, y: mid },
                b.clone(),
            ]);
        } else {
            let mid = (a.x + b.x) / 2.0;
            out.push(vec![
                a.clone(),
                Point { x: mid, y: a.y },
                Point { x: mid, y: b.y },
                b.clone(),
            ]);
        }
    }
    // Gutter detours. On a grid, two entities several cells apart have other
    // entities BETWEEN them, and every shape above goes directly -- so the best
    // available path still crossed a box. The grid's gaps are uniform empty
    // lanes, so a path can leave the direct line, run the gutter clear of
    // everything, and come back. This is what the bend cost is for.
    let gap_y = GRID_GAP_Y / 2.0;
    let gap_x = GRID_GAP_X / 2.0;
    let above = ba.y.min(bb.y) - gap_y;
    let below = (ba.y + ba.height).max(bb.y + bb.height) + gap_y;
    let left = ba.x.min(bb.x) - gap_x;
    let right = (ba.x + ba.width).max(bb.x + bb.width) + gap_x;

    for via_y in [above, below] {
        out.push(vec![
            a.clone(),
            Point { x: a.x, y: via_y },
            Point { x: b.x, y: via_y },
            b.clone(),
        ]);
    }
    for via_x in [left, right] {
        out.push(vec![
            a.clone(),
            Point { x: via_x, y: a.y },
            Point { x: via_x, y: b.y },
            b.clone(),
        ]);
    }
    out
}

/// Score a candidate path with the crate's established mount weights.
///
/// The weights are not re-invented here: `MOUNT_COST` already encodes that a
/// box traversal is fatal (1e9), a crossing is expensive (3000), a bend is real
/// but affordable (900), and length is a tiebreaker (6). Using them keeps ER
/// consistent with how every other diagram in the product is judged.
fn route_cost(
    path: &[Point],
    self_pi: usize,
    from_i: usize,
    to_i: usize,
    boxes: &[ErBox],
    others: &[Vec<Point>],
    pending: &[(usize, usize, &ErRelationshipInput)],
) -> f64 {
    let mut cost = 0.0;

    // Length and bends.
    let mut len = 0.0;
    for w in path.windows(2) {
        len += ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
    }
    cost += len * MOUNT_COST.length;
    cost += (path.len().saturating_sub(2)) as f64 * MOUNT_COST.bend;

    // Passing through any box that is not one of its own endpoints. This is the
    // term the greedy version violated ten times over.
    for (bi, b) in boxes.iter().enumerate() {
        if bi == from_i || bi == to_i {
            continue;
        }
        if path.windows(2).any(|w| segment_hits_rect(&w[0], &w[1], b)) {
            cost += MOUNT_COST.collision;
        }
    }

    // Crossing another edge, ignoring pairs that share an entity: those meet at
    // a box by construction, not in open space.
    for (oi, other) in others.iter().enumerate() {
        if oi == self_pi || other.len() < 2 {
            continue;
        }
        let (oi_from, oi_to, _) = pending[oi];
        if oi_from == from_i || oi_to == to_i || oi_from == to_i || oi_to == from_i {
            continue;
        }
        let crossings = path
            .windows(2)
            .flat_map(|w| other.windows(2).map(move |o| (w, o)))
            .filter(|(w, o)| segment_intersection(&w[0], &w[1], &o[0], &o[1]).is_some())
            .count();
        cost += crossings as f64 * MOUNT_COST.crossing;
    }

    cost
}

/// Whether a segment touches a rectangle's interior, with a little clearance so
/// a line does not graze a box it is meant to be avoiding.
fn segment_hits_rect(p: &Point, q: &Point, b: &ErBox) -> bool {
    let (x0, y0) = (b.x - DETOUR_CLEARANCE, b.y - DETOUR_CLEARANCE);
    let (x1, y1) = (
        b.x + b.width + DETOUR_CLEARANCE,
        b.y + b.height + DETOUR_CLEARANCE,
    );
    let inside = |pt: &Point| pt.x > x0 && pt.x < x1 && pt.y > y0 && pt.y < y1;
    if inside(p) || inside(q) {
        return true;
    }
    let corners = [
        (Point { x: x0, y: y0 }, Point { x: x1, y: y0 }),
        (Point { x: x1, y: y0 }, Point { x: x1, y: y1 }),
        (Point { x: x1, y: y1 }, Point { x: x0, y: y1 }),
        (Point { x: x0, y: y1 }, Point { x: x0, y: y0 }),
    ];
    corners
        .iter()
        .any(|(c, d)| segment_intersection(p, q, c, d).is_some())
}

/// Build every edge for a set of PLACED boxes.
///
/// Separate from `plan_er` so it can be re-run when a box moves without
/// redoing placement. Dragging an entity has to re-pick faces, redistribute
/// ports and recompute hops -- all of which is layout, so it lives here rather
/// than being reimplemented in the viewer.
pub fn route_edges(boxes: &[ErBox], input: &ErInput) -> Vec<ErEdge> {
    let entities = &input.entities;
    let index_of: BTreeMap<&str, usize> =
        entities.iter().enumerate().map(|(i, e)| (e.id.as_str(), i)).collect();

    //
    // Endpoints are DISTRIBUTED along the box edge rather than pinned to its
    // vertical midpoint. Pinning them meant a hub's edges all left from one
    // identical point: on Architext's own model eight edges shared a single
    // pixel, overlapping for their whole first run with eight crow's feet drawn
    // on top of each other. The box edge already has the height to separate
    // them, so nothing but the midpoint convention was in the way.
    let mut pending: Vec<(usize, usize, &ErRelationshipInput)> = Vec::new();
    for (i, entity) in entities.iter().enumerate() {
        for rel in &entity.relationships {
            if let Some(&j) = index_of.get(rel.to.as_str()) {
                pending.push((i, j, rel)); // dangling targets already rejected by validation
            }
        }
    }

    // --- surface selection: scored per PAIR, not guessed per box ------------
    //
    // Which face a line leaves from cannot be decided box by box. Picking the
    // face that merely points at the other entity is a greedy local choice, and
    // on Architext's own model it drove ten of sixteen lines straight THROUGH
    // other entities -- the one thing the cost model treats as fatal
    // (`MOUNT_COST.collision` is 1e9).
    //
    // So each edge is scored over all sixteen face pairs and a few path shapes
    // each, against the crate's existing weights: a box traversal is
    // effectively forbidden, a crossing costs 3000, a bend 900, and length 6
    // per unit. A bend is not banned and not free -- it is worth paying when it
    // buys avoiding a crossing or a box, which is exactly what those weights
    // say.
    //
    // Cost depends on where the OTHER edges went, so this iterates: every edge
    // re-picks against the current choices of the rest, in a fixed order, for a
    // bounded number of passes.
    let anchor = |b: &ErBox, side: Side| -> Point {
        match side {
            Side::Right => Point { x: b.x + b.width, y: b.y + b.height / 2.0 },
            Side::Left => Point { x: b.x, y: b.y + b.height / 2.0 },
            Side::Bottom => Point { x: b.x + b.width / 2.0, y: b.y + b.height },
            Side::Top => Point { x: b.x + b.width / 2.0, y: b.y },
        }
    };

    let mut sides: Vec<(Side, Side)> = pending
        .iter()
        .map(|&(i, j, _)| {
            let (a, b) = (&boxes[i], &boxes[j]);
            let dx = (b.x + b.width / 2.0) - (a.x + a.width / 2.0);
            let dy = (b.y + b.height / 2.0) - (a.y + a.height / 2.0);
            let from = if dx.abs() >= dy.abs() {
                if dx >= 0.0 { Side::Right } else { Side::Left }
            } else if dy >= 0.0 {
                Side::Bottom
            } else {
                Side::Top
            };
            (from, from.opposite())
        })
        .collect();

    let mut paths: Vec<Vec<Point>> = pending
        .iter()
        .enumerate()
        .map(|(pi, &(i, j, _))| {
            if i == j {
                return self_loop_path(&boxes[i]);
            }
            let (sa, sb) = sides[pi];
            candidate_shapes(&anchor(&boxes[i], sa), &anchor(&boxes[j], sb), sa, sb, &boxes[i], &boxes[j])
                .into_iter()
                .next()
                .unwrap_or_default()
        })
        .collect();

    for _ in 0..SURFACE_PASSES {
        let mut improved = false;
        for pi in 0..pending.len() {
            let (i, j, _) = pending[pi];
            if i == j {
                continue; // its loop is fixed geometry, not a face choice
            }
            let mut best: Option<(f64, (Side, Side), Vec<Point>)> = None;
            for &sa in &SIDES {
                for &sb in &SIDES {
                    let (pa, pb) = (anchor(&boxes[i], sa), anchor(&boxes[j], sb));
                    for shape in candidate_shapes(&pa, &pb, sa, sb, &boxes[i], &boxes[j]) {
                        let c = route_cost(&shape, pi, i, j, boxes, &paths, &pending);
                        // Strict improvement only, with the FIRST candidate in a
                        // fixed enumeration order winning ties, so the result
                        // cannot depend on evaluation order.
                        if best.as_ref().is_none_or(|(bc, _, _)| c < *bc - 1e-9) {
                            best = Some((c, (sa, sb), shape));
                        }
                    }
                }
            }
            if let Some((_, chosen_sides, chosen_path)) = best {
                if chosen_sides != sides[pi] {
                    improved = true;
                }
                sides[pi] = chosen_sides;
                paths[pi] = chosen_path;
            }
        }
        if !improved {
            break;
        }
    }

    // --- mount order on each surface ----------------------------------------
    //
    // Ports are not just spread along a face, they are ORDERED. Two lines
    // leaving the same face cross each other at the box unless the one whose
    // target sits further along the face's tangent mounts further along it too.
    // Sorting by that projection is what makes a hub's fan open cleanly instead
    // of braiding at the surface.
    let mut slots: BTreeMap<(usize, Side), Vec<usize>> = BTreeMap::new();
    for (pi, &(i, j, _)) in pending.iter().enumerate() {
        if i == j {
            continue; // routed as a fixed loop, so it mounts no shared port
        }
        slots.entry((i, sides[pi].0)).or_default().push(pi);
        slots.entry((j, sides[pi].1)).or_default().push(pi);
    }

    let mut ports: Vec<(Point, Point)> = vec![
        (Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 0.0 });
        pending.len()
    ];
    for ((owner, side), mut members) in slots {
        let b = &boxes[owner];
        let horizontal = side.is_horizontal_face();
        // Project the far end onto the face's tangent, then mount in that
        // order. Tie-broken by entity id so the layout is reproducible.
        members.sort_by(|&a, &b2| {
            let key = |pi: usize| {
                let (i, j, _) = pending[pi];
                let other = if i == owner { j } else { i };
                let ob = &boxes[other];
                let along = if horizontal {
                    ob.x + ob.width / 2.0
                } else {
                    ob.y + ob.height / 2.0
                };
                (along, pending[pi].2.to.as_str())
            };
            let (ka, ida) = key(a);
            let (kb, idb) = key(b2);
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal).then(ida.cmp(idb))
        });

        let extent = if horizontal { b.width } else { b.height };
        let span = (extent - PORT_INSET * 2.0).max(0.0);
        let n_ports = members.len();
        for (k, pi) in members.into_iter().enumerate() {
            let along = PORT_INSET + span * (k as f64 + 1.0) / (n_ports as f64 + 1.0);
            let point = match side {
                Side::Right => Point { x: b.x + b.width, y: b.y + along },
                Side::Left => Point { x: b.x, y: b.y + along },
                Side::Bottom => Point { x: b.x + along, y: b.y + b.height },
                Side::Top => Point { x: b.x + along, y: b.y },
            };
            let (i, _, _) = pending[pi];
            let unset = ports[pi].0.x == 0.0 && ports[pi].0.y == 0.0;
            if i == owner && sides[pi].0 == side && unset {
                ports[pi].0 = point;
            } else {
                ports[pi].1 = point;
            }
        }
    }

    // Re-pick the path shape for the REAL ports. Sides and mount order are
    // settled; this only chooses how to get between the two points now that
    // they have moved off the face centres the scoring used.
    for pi in 0..pending.len() {
        let (i, j, _) = pending[pi];
        if i == j {
            paths[pi] = self_loop_path(&boxes[i]);
            continue;
        }
        let (sa, sb) = sides[pi];
        let mut best: Option<(f64, Vec<Point>)> = None;
        for shape in candidate_shapes(&ports[pi].0, &ports[pi].1, sa, sb, &boxes[i], &boxes[j]) {
            let c = route_cost(&shape, pi, i, j, boxes, &paths, &pending);
            if best.as_ref().is_none_or(|(bc, _)| c < *bc - 1e-9) {
                best = Some((c, shape));
            }
        }
        if let Some((_, shape)) = best {
            paths[pi] = shape;
        }
    }

    let mut edges = Vec::new();
    for (pi, &(i, _, rel)) in pending.iter().enumerate() {
        let (from_foot, to_foot) = feet(&rel.cardinality);
        let points = paths[pi].clone();
        // Width the pill will occupy, so the clearance test matches what the
        // viewer actually draws rather than a bare point.
        let half_w = rel
            .label
            .as_deref()
            .map_or(0.0, |t| (t.chars().count() as f64 * CHAR_W + 12.0) / 2.0);
        let (label_x, label_y) = label_anchor(&points, boxes, half_w);
        edges.push(ErEdge {
            from: entities[i].id.clone(),
            to: rel.to.clone(),
            label: rel.label.clone(),
            cardinality: rel.cardinality.clone(),
            points,
            from_foot,
            to_foot,
            label_x,
            label_y,
            hops: Vec::new(),
        });
    }

    let hops = hop_points(&edges);
    for (e, h) in edges.iter_mut().zip(hops) {
        e.hops = h;
    }
    edges
}

/// Where an edge passes over one drawn before it.
///
/// Crossings are unavoidable in a graph that is not planar, and two lines
/// meeting at a point are ambiguous: a reader cannot tell whether they cross or
/// join. A hop resolves it. Computed here rather than in the viewer so the
/// choice of which line hops is deterministic -- always the later edge, by a
/// fixed order -- instead of depending on paint order.
fn hop_points(edges: &[ErEdge]) -> Vec<Vec<Point>> {
    let mut hops: Vec<Vec<Point>> = vec![Vec::new(); edges.len()];
    for i in 0..edges.len() {
        for j in 0..i {
            // Edges sharing an entity meet at a box, not in open space.
            if edges[i].from == edges[j].from
                || edges[i].to == edges[j].to
                || edges[i].from == edges[j].to
                || edges[i].to == edges[j].from
            {
                continue;
            }
            if let Some(p) = segment_intersection(
                &edges[i].points[0],
                &edges[i].points[1],
                &edges[j].points[0],
                &edges[j].points[1],
            ) {
                hops[i].push(p);
            }
        }
        // Along the line, so the renderer can walk them in order.
        let origin = edges[i].points[0].clone();
        hops[i].sort_by(|a, b| {
            let da = (a.x - origin.x).powi(2) + (a.y - origin.y).powi(2);
            let db = (b.x - origin.x).powi(2) + (b.y - origin.y).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    hops
}

/// Do two segments properly cross, in plain (x, y) pairs?
///
/// Used on CELL coordinates while arranging the grid, before any real geometry
/// exists.
fn segments_cross_xy(a1: (f64, f64), a2: (f64, f64), b1: (f64, f64), b2: (f64, f64)) -> bool {
    let o = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    let (d1, d2) = (o(a1, a2, b1), o(a1, a2, b2));
    let (d3, d4) = (o(b1, b2, a1), o(b1, b2, a2));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// Intersection of two segments, or None when they do not properly cross.
fn segment_intersection(p1: &Point, p2: &Point, p3: &Point, p4: &Point) -> Option<Point> {
    let (r1, r2) = (p2.x - p1.x, p2.y - p1.y);
    let (s1, s2) = (p4.x - p3.x, p4.y - p3.y);
    let denom = r1 * s2 - r2 * s1;
    if denom.abs() < 1e-9 {
        return None; // parallel or collinear
    }
    let t = ((p3.x - p1.x) * s2 - (p3.y - p1.y) * s1) / denom;
    let u = ((p3.x - p1.x) * r2 - (p3.y - p1.y) * r1) / denom;
    // Strictly interior, so a line touching another's endpoint is not a hop.
    if (0.02..=0.98).contains(&t) && (0.02..=0.98).contains(&u) {
        Some(Point { x: p1.x + t * r1, y: p1.y + t * r2 })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, ty: &str) -> ErAttributeInput {
        ErAttributeInput { name: name.into(), type_name: ty.into(), ..Default::default() }
    }

    fn entity(id: &str, attrs: usize, rels: &[(&str, &str)]) -> ErEntityInput {
        ErEntityInput {
            id: id.into(),
            name: id.to_uppercase(),
            attributes: (0..attrs).map(|k| attr(&format!("col{k}"), "text")).collect(),
            relationships: rels
                .iter()
                .map(|(to, card)| ErRelationshipInput {
                    to: (*to).into(),
                    cardinality: (*card).into(),
                    label: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// A hub referenced by five others -- the shape a grid layout handles worst
    /// and the reason this engine layers instead.
    fn hub_input() -> ErInput {
        let mut entities = vec![entity(
            "account",
            6,
            &[
                ("order", "one-to-many"),
                ("invoice", "one-to-many"),
                ("address", "one-to-many"),
                ("session", "one-to-many"),
                ("audit_log", "one-to-many"),
            ],
        )];
        for id in ["order", "invoice", "address", "session", "audit_log"] {
            entities.push(entity(id, 3, &[]));
        }
        ErInput { entities }
    }

    #[test]
    fn box_height_tracks_attribute_count() {
        // WHY: this is the property that forced a separate engine. If box height
        // ever stops depending on attribute count, plan_er has silently become
        // plan_diagram with extra steps.
        let plan = plan_er(&ErInput {
            entities: vec![entity("small", 2, &[]), entity("large", 15, &[])],
        });
        let small = plan.boxes.iter().find(|b| b.id == "small").unwrap();
        let large = plan.boxes.iter().find(|b| b.id == "large").unwrap();
        assert!(
            large.height > small.height,
            "a 15-attribute entity must be taller than a 2-attribute one: {} vs {}",
            large.height,
            small.height
        );
        assert_eq!(large.height - small.height, 13.0 * ROW_H);
    }

    #[test]
    fn layout_is_deterministic() {
        // WHY: the crate's non-negotiable. A layout that varies between runs
        // makes every downstream diff and every fitness number meaningless.
        let input = hub_input();
        let a = plan_er(&input);
        let b = plan_er(&input);
        assert_eq!(a, b, "same input must produce a byte-identical plan");
    }

    #[test]
    fn declared_relationship_is_distinguished_from_bare_foreign_key() {
        // WHY: a foreign key with no matching relationship is valid and draws
        // no edge. The viewer can only annotate that gap if the plan tells it
        // which rows are in it.
        let mut e = entity("release", 0, &[("release_item", "one-to-many")]);
        e.attributes = vec![
            ErAttributeInput {
                name: "item_id".into(),
                type_name: "uuid".into(),
                key: Some("foreign".into()),
                references: Some("release_item".into()),
                required: false,
            },
            ErAttributeInput {
                name: "plan_id".into(),
                type_name: "uuid".into(),
                key: Some("foreign".into()),
                references: Some("plan".into()),
                required: false,
            },
        ];
        let plan = plan_er(&ErInput {
            entities: vec![e, entity("release_item", 1, &[]), entity("plan", 1, &[])],
        });
        let rows = &plan.boxes.iter().find(|b| b.id == "release").unwrap().rows;
        assert!(rows[0].relationship_declared, "item_id has a declared relationship");
        assert!(!rows[1].relationship_declared, "plan_id has none and draws no edge");
    }

    /// Two hubs over the same three children, declared in OPPOSITE orders.
    ///
    /// The smallest shape where placement decides whether edges cross: a single
    /// hub's fan cannot cross itself, because every edge shares the hub.
    fn shared_children_input() -> ErInput {
        ErInput {
            entities: vec![
                entity(
                    "account",
                    4,
                    &[
                        ("order", "one-to-many"),
                        ("invoice", "one-to-many"),
                        ("address", "one-to-many"),
                    ],
                ),
                entity("order", 3, &[]),
                entity("invoice", 3, &[]),
                entity("address", 3, &[]),
                entity(
                    "vendor",
                    4,
                    &[
                        ("address", "one-to-many"),
                        ("invoice", "one-to-many"),
                        ("order", "one-to-many"),
                    ],
                ),
            ],
        }
    }

    fn realistic_fixture_input() -> ErInput {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/entities-viewer/docs/architext/data/entities.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("fixture unreadable at {}: {e}", path.display()));
        serde_json::from_str(&text).expect("fixture parses as ErInput")
    }

    #[test]
    fn related_entities_cluster_and_unrelated_ones_stay_apart() {
        // THE claim that justifies dropping columns. Columns encoded nothing a
        // reader could use; proximity has to earn its place by actually meaning
        // something. Two disconnected triangles: every within-cluster distance
        // must be smaller than every between-cluster distance, or "related
        // things are near each other" is not true and the layout says nothing.
        let mut entities = vec![
            entity("a1", 3, &[("a2", "one-to-many"), ("a3", "one-to-many")]),
            entity("a2", 3, &[("a3", "one-to-many")]),
            entity("a3", 3, &[]),
            entity("b1", 3, &[("b2", "one-to-many"), ("b3", "one-to-many")]),
            entity("b2", 3, &[("b3", "one-to-many")]),
            entity("b3", 3, &[]),
        ];
        entities.rotate_left(1); // input order must not decide the outcome
        let plan = plan_er(&ErInput { entities });
        let centre = |id: &str| {
            let b = plan.boxes.iter().find(|b| b.id == id).unwrap();
            (b.x + b.width / 2.0, b.y + b.height / 2.0)
        };
        let dist = |p: (f64, f64), q: (f64, f64)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
        let a = ["a1", "a2", "a3"];
        let b = ["b1", "b2", "b3"];
        let within = a
            .iter()
            .flat_map(|x| a.iter().map(move |y| (x, y)))
            .chain(b.iter().flat_map(|x| b.iter().map(move |y| (x, y))))
            .filter(|(x, y)| x != y)
            .map(|(x, y)| dist(centre(x), centre(y)))
            .fold(0.0_f64, f64::max);
        let between = a
            .iter()
            .flat_map(|x| b.iter().map(move |y| (x, y)))
            .map(|(x, y)| dist(centre(x), centre(y)))
            .fold(f64::INFINITY, f64::min);
        // On a grid, clusters occupy contiguous BLOCKS and adjacent blocks
        // touch, so the widest within-cluster pair is not required to beat the
        // nearest cross-cluster pair -- that was a force-layout property. What
        // must hold is that related entities are closer ON AVERAGE, which is
        // what "related things sit together" actually means here.
        let mean = |v: Vec<f64>| v.iter().sum::<f64>() / v.len() as f64;
        let within_mean = mean(
            a.iter()
                .flat_map(|x| a.iter().map(move |y| (x, y)))
                .chain(b.iter().flat_map(|x| b.iter().map(move |y| (x, y))))
                .filter(|(x, y)| x != y)
                .map(|(x, y)| dist(centre(x), centre(y)))
                .collect(),
        );
        let between_mean = mean(
            a.iter()
                .flat_map(|x| b.iter().map(move |y| (x, y)))
                .map(|(x, y)| dist(centre(x), centre(y)))
                .collect(),
        );
        assert!(
            within_mean < between_mean,
            "related entities should average closer ({within_mean:.0}) than unrelated \
             ({between_mean:.0}); widest-within was {within:.0}, nearest-between {between:.0}"
        );
    }

    #[test]
    fn boxes_never_overlap() {
        // The invariant free placement has to hold that a grid got for free.
        // Repulsion treats boxes as points, so overlap is resolved explicitly;
        // if that pass regresses, entities render on top of each other.
        for (name, input) in [
            ("hub", hub_input()),
            ("shared children", shared_children_input()),
            ("realistic fixture", realistic_fixture_input()),
        ] {
            let plan = plan_er(&input);
            for i in 0..plan.boxes.len() {
                for j in (i + 1)..plan.boxes.len() {
                    let (a, b) = (&plan.boxes[i], &plan.boxes[j]);
                    let overlap = a.x < b.x + b.width
                        && b.x < a.x + a.width
                        && a.y < b.y + b.height
                        && b.y < a.y + a.height;
                    assert!(!overlap, "{name}: {} overlaps {}", a.id, b.id);
                }
            }
        }
    }

    #[test]
    fn measure_distribution() {
        for (name, input) in [("fixture", realistic_fixture_input())] {
            let plan = plan_er(&input);
            let c: Vec<(f64, f64)> = plan.boxes.iter()
                .map(|b| (b.x + b.width / 2.0, b.y + b.height / 2.0)).collect();
            // nearest-neighbour distance per box
            let nn: Vec<f64> = c.iter().enumerate().map(|(i, p)| {
                c.iter().enumerate().filter(|(j, _)| *j != i)
                 .map(|(_, q)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt())
                 .fold(f64::INFINITY, f64::min)
            }).collect();
            let mean = nn.iter().sum::<f64>() / nn.len() as f64;
            let sd = (nn.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / nn.len() as f64).sqrt();
            // how much of the canvas is genuinely occupied, by 4x4 cell coverage
            let (cw, ch) = (plan.canvas_width, plan.canvas_height);
            let mut cells = vec![false; 16];
            for b in &plan.boxes {
                for gx in 0..4 { for gy in 0..4 {
                    let (x0, x1) = (cw * gx as f64 / 4.0, cw * (gx + 1) as f64 / 4.0);
                    let (y0, y1) = (ch * gy as f64 / 4.0, ch * (gy + 1) as f64 / 4.0);
                    if b.x < x1 && x0 < b.x + b.width && b.y < y1 && y0 < b.y + b.height {
                        cells[gy * 4 + gx] = true;
                    }
                }}
            }
            let occupied = cells.iter().filter(|c| **c).count();
            let area: f64 = plan.boxes.iter().map(|b| b.width * b.height).sum();
            println!("DIST {name}: {:.0}x{:.0} fill {:.3} nn_mean {:.0} nn_cv {:.2} cells {}/16",
                cw, ch, area / (cw * ch), mean, sd / mean, occupied);
        }
    }

    #[test]
    fn nothing_is_laid_out_beyond_the_canvas() {
        // REGRESSION: the canvas was measured from BOX extents only, while
        // routes deliberately leave their boxes -- a self-loop reaches 38px
        // above and right of its entity, a gutter detour runs outside the outer
        // rows. Anything on the top or left row landed at NEGATIVE coordinates:
        // outside the viewBox, clipped, and unreachable by scrolling, because
        // there is nothing to scroll to.
        for (name, input) in [
            ("fixture", realistic_fixture_input()),
            ("hub", hub_input()),
            ("shared children", shared_children_input()),
        ] {
            let plan = plan_er(&input);
            let (w, h) = (plan.canvas_width, plan.canvas_height);
            for b in &plan.boxes {
                assert!(
                    b.x >= 0.0 && b.y >= 0.0 && b.x + b.width <= w && b.y + b.height <= h,
                    "{name}: entity {} at ({},{}) {}x{} falls outside the {w}x{h} canvas",
                    b.id, b.x, b.y, b.width, b.height
                );
            }
            for e in &plan.edges {
                for p in e.points.iter().chain(e.hops.iter()) {
                    assert!(
                        p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= h,
                        "{name}: {} -> {} passes through ({}, {}), outside the {w}x{h} canvas",
                        e.from, e.to, p.x, p.y
                    );
                }
            }
        }
    }

    #[test]
    fn the_layout_is_balanced() {
        // The property the grid exists for, and the one fill ratio could not
        // express. Fill ratio is MAXIMISED by bunching -- boxes crushed into a
        // corner score beautifully on it -- so it was possible to pass every
        // test while the diagram had a dense core and a dead rim.
        //
        // Balance is measured two ways: spacing between neighbours should be
        // near-uniform, and no quarter of the canvas should be empty.
        for (name, input) in [("fixture", realistic_fixture_input()), ("hub", hub_input())] {
            let plan = plan_er(&input);
            let c: Vec<(f64, f64)> = plan
                .boxes
                .iter()
                .map(|b| (b.x + b.width / 2.0, b.y + b.height / 2.0))
                .collect();
            let nn: Vec<f64> = c
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    c.iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, q)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt())
                        .fold(f64::INFINITY, f64::min)
                })
                .collect();
            let mean = nn.iter().sum::<f64>() / nn.len() as f64;
            let sd = (nn.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / nn.len() as f64).sqrt();
            let cv = sd / mean;
            assert!(
                cv <= 0.35,
                "{name}: nearest-neighbour spacing varies by {cv:.2} of its mean; \
                 the layout is clumping rather than spreading"
            );

            // Every quadrant carries something. A layout can be evenly spaced
            // and still occupy only half the canvas.
            let (cw, ch) = (plan.canvas_width, plan.canvas_height);
            for (qx, qy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (x0, x1) = (cw * qx as f64 / 2.0, cw * (qx + 1) as f64 / 2.0);
                let (y0, y1) = (ch * qy as f64 / 2.0, ch * (qy + 1) as f64 / 2.0);
                let occupied = plan.boxes.iter().any(|b| {
                    b.x < x1 && x0 < b.x + b.width && b.y < y1 && y0 < b.y + b.height
                });
                assert!(occupied, "{name}: quadrant ({qx},{qy}) of the canvas is empty");
            }
        }
    }

    #[test]
    fn the_layout_stays_compact() {
        // THE test that was missing when free placement first shipped, and the
        // reason a broken layout reached the browser looking green.
        //
        // Without a displacement cap the simulation drifted apart every tick:
        // the fixture settled at 6742x3099 with 1.7% of the canvas covered, and
        // rendered as specks once the SVG scaled to fit. Every existing test
        // still passed, because all of them checked RELATIVE properties --
        // clusters were still clusters, boxes still did not overlap, the aspect
        // ratio was still fine. Nothing measured whether the diagram was a
        // sensible SIZE.
        //
        // Fill ratio is the check that cannot be satisfied by an exploded
        // layout. The floor is set below the measured 0.23 so ordinary layout
        // changes do not trip it, and far above the 0.017 that shipped.
        const MIN_FILL: f64 = 0.15;
        for (name, input) in [("fixture", realistic_fixture_input()), ("hub", hub_input())] {
            let plan = plan_er(&input);
            let box_area: f64 = plan.boxes.iter().map(|b| b.width * b.height).sum();
            let fill = box_area / (plan.canvas_width * plan.canvas_height);
            assert!(
                fill >= MIN_FILL,
                "{name}: boxes cover {fill:.3} of a {:.0}x{:.0} canvas; the layout has \
                 spread out until the diagram is unreadable",
                plan.canvas_width,
                plan.canvas_height
            );
        }
    }

    #[test]
    fn the_canvas_is_not_a_tall_strip() {
        // The complaint that started this: layering produced 1473x1383 and
        // before that 1055x1335 -- a column of boxes to scroll through. Free
        // placement should use both axes.
        let plan = plan_er(&realistic_fixture_input());
        let aspect = plan.canvas_width / plan.canvas_height;
        assert!(
            (0.5..=2.5).contains(&aspect),
            "canvas {}x{} has aspect {aspect:.2}; it should read as a diagram, not a strip",
            plan.canvas_width,
            plan.canvas_height
        );
    }

    #[test]
    fn no_edge_is_routed_through_an_entity() {
        // REGRESSION, and the reason surface selection is scored rather than
        // guessed. Picking the face that merely points at the other box is a
        // greedy local choice: on Architext's own model it drove TEN of sixteen
        // lines straight through other entities -- violating the most expensive
        // term in the cost model (MOUNT_COST.collision is 1e9) ten times over.
        //
        // Checked with real segment-vs-rectangle intersection. A bounding-box
        // test passes diagonals it should catch, which is how the earlier
        // measurement read zero while the diagram was full of them.
        for (name, input) in [
            ("fixture", realistic_fixture_input()),
            ("hub", hub_input()),
            ("shared children", shared_children_input()),
        ] {
            let plan = plan_er(&input);
            for e in &plan.edges {
                for b in &plan.boxes {
                    if b.id == e.from || b.id == e.to {
                        continue;
                    }
                    let hits = e
                        .points
                        .windows(2)
                        .any(|w| segment_hits_rect_bare(&w[0], &w[1], b));
                    assert!(
                        !hits,
                        "{name}: {} -> {} is routed through {}",
                        e.from, e.to, b.id
                    );
                }
            }
        }
    }

    /// Segment vs the box itself, with no routing clearance -- the render-time
    /// question of whether a line visibly crosses a box.
    fn segment_hits_rect_bare(p: &Point, q: &Point, b: &ErBox) -> bool {
        let inside = |pt: &Point| {
            pt.x > b.x + 1.0
                && pt.x < b.x + b.width - 1.0
                && pt.y > b.y + 1.0
                && pt.y < b.y + b.height - 1.0
        };
        if inside(p) || inside(q) {
            return true;
        }
        let c = [
            (Point { x: b.x, y: b.y }, Point { x: b.x + b.width, y: b.y }),
            (
                Point { x: b.x + b.width, y: b.y },
                Point { x: b.x + b.width, y: b.y + b.height },
            ),
            (
                Point { x: b.x + b.width, y: b.y + b.height },
                Point { x: b.x, y: b.y + b.height },
            ),
            (Point { x: b.x, y: b.y + b.height }, Point { x: b.x, y: b.y }),
        ];
        c.iter().any(|(u, v)| segment_intersection(p, q, u, v).is_some())
    }

    #[test]
    fn a_hubs_edges_leave_from_distinct_points() {
        // REGRESSION, found by dogfooding: every endpoint was pinned to the
        // box's vertical midpoint, so all eight of `node`'s edges started at one
        // identical pixel -- overlapping for their whole first run, with eight
        // crow's feet drawn on top of each other. The box edge has the height to
        // separate them; only the midpoint convention was in the way.
        let plan = plan_er(&hub_input());
        let starts: Vec<(i64, i64)> = plan
            .edges
            .iter()
            .map(|e| ((e.points[0].x * 100.0) as i64, (e.points[0].y * 100.0) as i64))
            .collect();
        let distinct: std::collections::BTreeSet<_> = starts.iter().collect();
        assert_eq!(
            distinct.len(),
            starts.len(),
            "each of the hub's {} edges needs its own exit point; got {} distinct",
            starts.len(),
            distinct.len()
        );

        // ...and the ports must stay ON the box edge, not drift past its corners.
        let hub = plan.boxes.iter().find(|b| b.id == "account").unwrap();
        for e in &plan.edges {
            let y = e.points[0].y;
            assert!(
                y >= hub.y && y <= hub.y + hub.height,
                "port {y} is outside the hub box ({}..{})",
                hub.y,
                hub.y + hub.height
            );
        }
    }

    #[test]
    fn unrelated_entities_settle_outside_the_connected_cluster() {
        // Under columns these piled into the hub's column and made it taller.
        // With free placement nothing pins them anywhere, so the property worth
        // holding is that repulsion carries them clear of the connected group
        // rather than leaving them sitting in the middle of it.
        let mut entities = vec![
            entity("hub", 3, &[("leaf1", "one-to-many"), ("leaf2", "one-to-many")]),
            entity("leaf1", 3, &[]),
            entity("leaf2", 3, &[]),
        ];
        for id in ["rule", "glossary", "note"] {
            entities.push(entity(id, 3, &[]));
        }
        let plan = plan_er(&ErInput { entities });
        let centre = |id: &str| {
            let b = plan.boxes.iter().find(|b| b.id == id).unwrap();
            (b.x + b.width / 2.0, b.y + b.height / 2.0)
        };
        let d = |id: &str| {
            let (hx, hy) = centre("hub");
            let (x, y) = centre(id);
            ((x - hx).powi(2) + (y - hy).powi(2)).sqrt()
        };
        // On a grid they take the TRAILING cells rather than being flung to the
        // rim, so the property is about reading order, not distance: everything
        // with structure to show is laid out first, and the standalone entities
        // fill what is left. Cell order reads left-to-right, top-to-bottom, so
        // "later" means further down, or further right on the same row.
        let after = |id: &str, other: &str| {
            let (p, q) = (centre(id), centre(other));
            p.1 > q.1 + 1.0 || ((p.1 - q.1).abs() <= 1.0 && p.0 > q.0)
        };
        for id in ["rule", "glossary", "note"] {
            for connected in ["hub", "leaf1", "leaf2"] {
                assert!(
                    after(id, connected),
                    "{id} has no relationships and should be laid out after {connected}"
                );
            }
        }
        let _ = d("leaf1");
    }

    #[test]
    fn a_box_is_wide_enough_for_its_widest_rendered_row() {
        // REGRESSION: found by measuring the rendered SVG, not by a unit test.
        // The width estimate ignored both the undeclared marker and the 32px
        // key-column offset the renderer uses, so an annotated row overflowed
        // its box by 153px into the entity beside it.
        //
        // This asserts the estimate against the SAME constants the renderer
        // lays out with, which is what makes it capable of catching a repeat.
        let e = ErEntityInput {
            id: "category".into(),
            name: "Category".into(),
            attributes: vec![ErAttributeInput {
                name: "parent_id".into(),
                type_name: "uuid".into(),
                key: Some("foreign".into()),
                references: Some("category".into()),
                required: false,
            }],
            ..Default::default()
        };
        let plan = plan_er(&ErInput { entities: vec![e] });
        let b = &plan.boxes[0];
        assert!(!b.rows[0].relationship_declared, "self-reference declares nothing");

        let rendered = ROW_TEXT_X
            + (b.rows[0].name.chars().count()
                + 2
                + b.rows[0].type_name.chars().count()
                + b.rows[0].references.as_ref().unwrap().chars().count()
                + REF_SEPARATOR_CHARS
                + UNDECLARED_MARKER.chars().count()) as f64
                * CHAR_W;
        assert!(
            b.width >= rendered.min(BOX_MAX_W),
            "box {} wide cannot hold a {rendered} row",
            b.width
        );
    }

    #[test]
    fn a_foreign_key_is_declared_when_the_OTHER_end_declares_the_link() {
        // REGRESSION: found by rendering the fixture, not by a unit test.
        //
        // The original check looked only at the entity's own `relationships`.
        // In the normal parent-declares-children shape, the child holds the
        // foreign key and the PARENT holds the relationship, so every child's
        // foreign key was annotated "no relationship declared" while its edge
        // was drawn plainly on the diagram. 10 of 11 flags in a 14-entity
        // fixture were wrong.
        //
        // The earlier test missed it because it put the foreign key and the
        // relationship on the same entity, which is the one arrangement where
        // both readings agree.
        let parent = ErEntityInput {
            id: "account".into(),
            name: "Account".into(),
            attributes: vec![attr("id", "uuid")],
            relationships: vec![ErRelationshipInput {
                to: "order".into(),
                cardinality: "one-to-many".into(),
                label: None,
            }],
            ..Default::default()
        };
        let child = ErEntityInput {
            id: "order".into(),
            name: "Order".into(),
            attributes: vec![
                attr("id", "uuid"),
                ErAttributeInput {
                    name: "account_id".into(),
                    type_name: "uuid".into(),
                    key: Some("foreign".into()),
                    references: Some("account".into()),
                    required: true,
                },
                // Self-reference with no relationship at either end: this one
                // genuinely draws no edge and MUST stay flagged, so the fix
                // cannot be "never flag anything".
                ErAttributeInput {
                    name: "parent_order_id".into(),
                    type_name: "uuid".into(),
                    key: Some("foreign".into()),
                    references: Some("order".into()),
                    required: false,
                },
            ],
            ..Default::default()
        };
        let plan = plan_er(&ErInput { entities: vec![parent, child] });
        let rows = &plan.boxes.iter().find(|b| b.id == "order").unwrap().rows;
        assert!(
            rows[1].relationship_declared,
            "account_id is backed by the edge account -> order and must not be flagged"
        );
        assert!(
            !rows[2].relationship_declared,
            "parent_order_id has no relationship at either end and must stay flagged"
        );
    }

    #[test]
    fn crossing_counter_can_detect_a_crossing() {
        // GUARD: proves the fitness metric below is capable of failing. A
        // counter that always returns 0 would make every fitness assertion pass
        // while measuring nothing -- the failure mode this whole test exists to
        // rule out. Two edges between four distinct entities, routed as an X.
        let p = |x: f64, y: f64| Point { x, y };
        let plan = ErPlan {
            boxes: Vec::new(),
            edges: vec![
                ErEdge {
                    from: "a".into(),
                    to: "d".into(),
                    label: None,
                    cardinality: "one-to-one".into(),
                    points: vec![p(0.0, 0.0), p(100.0, 100.0)],
                    from_foot: ErFoot::One,
                    to_foot: ErFoot::One,
                    label_x: 0.0,
                    label_y: 0.0,
                    hops: Vec::new(),
                },
                ErEdge {
                    from: "b".into(),
                    to: "c".into(),
                    label: None,
                    cardinality: "one-to-one".into(),
                    points: vec![p(0.0, 100.0), p(100.0, 0.0)],
                    from_foot: ErFoot::One,
                    to_foot: ErFoot::One,
                    label_x: 0.0,
                    label_y: 0.0,
                    hops: Vec::new(),
                },
            ],
            canvas_width: 100.0,
            canvas_height: 100.0,
        };
        assert_eq!(count_crossings(&plan), 1, "an X must count as one crossing");
    }

    #[test]
    fn shared_children_stay_within_their_crossing_budget() {
        // Two hubs over the same three children. A single hub's fan cannot
        // cross itself -- every edge shares the hub -- so this is the smallest
        // shape where placement genuinely decides the outcome.
        let plan = plan_er(&shared_children_input());
        let crossings = count_crossings(&plan);

        // ONE, not zero, and that is a deliberate trade rather than a defect
        // left unexplained.
        //
        // This is K(2,3): both hubs join all three children. Its zero-crossing
        // embedding puts the hubs at opposite ends with the children stacked
        // between them. Force placement optimises DISTANCE, not planarity, and
        // settles the hubs side by side with the children around them -- a
        // legitimate energy minimum that costs one crossing.
        //
        // Layering used to score 0 here, because a two-layer graph cannot do
        // otherwise; it paid for that with columns that meant nothing, a canvas
        // that read as a strip, and every edge sharing one channel. The
        // realistic 14-entity fixture scores 0 under free placement, so this is
        // a synthetic worst case rather than what a reader will meet.
        //
        // Still a ratchet: lower it if placement improves, never raise it.
        const BUDGET: usize = 1;
        assert!(
            crossings <= BUDGET,
            "crossings {crossings} exceeds budget {BUDGET}; offending: {:?}",
            crossing_pairs(&plan)
        );
    }

    #[test]
    fn realistic_fixture_stays_within_its_crossing_budget() {
        // Fitness on the 14-entity fixture -- the same data the viewer renders,
        // so this number and what a reader sees cannot drift apart.
        let input = realistic_fixture_input();
        assert_eq!(input.entities.len(), 14, "fixture size changed; revisit the budget");
        let plan = plan_er(&input);
        let crossings = count_crossings(&plan);

        // A RATCHET, not an aspiration: lower it when the layout improves,
        // never raise it to make a regression pass.
        const BUDGET: usize = 0;
        assert!(
            crossings <= BUDGET,
            "crossings {crossings} exceeds budget {BUDGET}; offending: {:?}",
            crossing_pairs(&plan)
        );
    }

    #[test]
    fn an_entirely_unrelated_model_still_lays_out() {
        // Every entity isolated means there is no connected height to match.
        // Without a fallback that divides by zero or stacks everything into one
        // very long column.
        let entities = (0..9).map(|i| entity(&format!("t{i}"), 3, &[])).collect();
        let plan = plan_er(&ErInput { entities });
        assert_eq!(plan.boxes.len(), 9);
        assert!(plan.edges.is_empty());
        let distinct_columns: std::collections::BTreeSet<i64> =
            plan.boxes.iter().map(|b| b.x as i64).collect();
        assert!(distinct_columns.len() > 1, "9 unrelated entities should not be one column");
        assert!(plan.canvas_height.is_finite() && plan.canvas_height > 0.0);
    }

    #[test]
    fn empty_input_is_a_valid_empty_plan() {
        let plan = plan_er(&ErInput::default());
        assert!(plan.boxes.is_empty() && plan.edges.is_empty());
        assert!(plan.canvas_width > 0.0 && plan.canvas_height > 0.0);
    }

    // --- crossing counter (test-only fitness helper) ------------------------

    fn crossing_pairs(plan: &ErPlan) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for a in 0..plan.edges.len() {
            for b in (a + 1)..plan.edges.len() {
                let (ea, eb) = (&plan.edges[a], &plan.edges[b]);
                if ea.from == eb.from || ea.to == eb.to || ea.from == eb.to || ea.to == eb.from {
                    continue;
                }
                let sa: Vec<_> = ea.points.windows(2).map(|w| (w[0].clone(), w[1].clone())).collect();
                let sb: Vec<_> = eb.points.windows(2).map(|w| (w[0].clone(), w[1].clone())).collect();
                if sa.iter().any(|s| sb.iter().any(|t| segments_cross(s, t))) {
                    out.push((format!("{}->{}", ea.from, ea.to), format!("{}->{}", eb.from, eb.to)));
                }
            }
        }
        out
    }

    fn count_crossings(plan: &ErPlan) -> usize {
        let segs: Vec<Vec<(Point, Point)>> = plan
            .edges
            .iter()
            .map(|e| {
                e.points.windows(2).map(|w| (w[0].clone(), w[1].clone())).collect::<Vec<_>>()
            })
            .collect();
        let mut crossings = 0;
        for a in 0..segs.len() {
            for b in (a + 1)..segs.len() {
                // Edges sharing an endpoint box meet by construction; only
                // count genuine crossings between different edge pairs.
                if plan.edges[a].from == plan.edges[b].from
                    || plan.edges[a].to == plan.edges[b].to
                    || plan.edges[a].from == plan.edges[b].to
                    || plan.edges[a].to == plan.edges[b].from
                {
                    continue;
                }
                for s in &segs[a] {
                    for t in &segs[b] {
                        if segments_cross(s, t) {
                            crossings += 1;
                        }
                    }
                }
            }
        }
        crossings
    }

    fn segments_cross(s: &(Point, Point), t: &(Point, Point)) -> bool {
        fn orient(p: &Point, q: &Point, r: &Point) -> f64 {
            (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x)
        }
        let d1 = orient(&s.0, &s.1, &t.0);
        let d2 = orient(&s.0, &s.1, &t.1);
        let d3 = orient(&t.0, &t.1, &s.0);
        let d4 = orient(&t.0, &t.1, &s.1);
        ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
    }
}
