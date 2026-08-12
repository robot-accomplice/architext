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

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::Point;

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
/// Minimum clear space between any two boxes.
const BOX_CLEARANCE: f64 = 44.0;
/// Preferred clear space between two boxes joined by a relationship. Related
/// entities sit closer than unrelated ones; that proximity is what carries the
/// structure now that there are no columns to read it from.
const IDEAL_EDGE_GAP: f64 = 55.0;

/// How far in from a box's top and bottom corners the outermost edge port sits,
/// so a line never appears to attach to the corner itself.
const PORT_INSET: f64 = 10.0;
/// Canvas margin on every side.
const MARGIN: f64 = 28.0;
/// Placement iterations. Fixed rather than convergence-based so the result is
/// identical every run; the layout is settled well before this at ER scale
/// (tens of entities), and the cost is trivial -- 60 entities is 3,600 pairs.
const PLACEMENT_TICKS: usize = 420;
/// Overlap-resolution passes per tick. Boxes are rectangles, so separation is
/// resolved directly rather than left to the repulsion term.
const SEPARATION_PASSES: usize = 4;
/// Pull toward the centroid, per unit of distance from it.
///
/// Repulsion acts between every pair, attraction only along relationships, so a
/// sparse model -- and a schema is sparse, sixteen entities with sixteen
/// relationships -- has far more push than pull and drifts apart. Gravity is
/// what closes that gap; without it the fixture settled at 6% of the canvas
/// covered, against 17% for the layout it replaced.
const GRAVITY: f64 = 1.1;
/// Golden angle, for the deterministic phyllotaxis seed positions. A spiral
/// spreads the starting points evenly without needing a random seed.
const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

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

    // --- placement: free 2D, no columns --------------------------------------
    //
    // ER relationships are an undirected graph with cycles. Layering them into
    // columns imposes a hierarchy the data does not have: the columns carry no
    // label, encode no category, and cannot be read for anything. They were
    // paid for in a tall canvas, a routing channel every edge had to share, and
    // ports crammed onto one face of each box -- all to satisfy a constraint
    // nothing asked for.
    //
    // Entities are placed freely instead. Related ones pull together, every
    // pair pushes apart, and overlapping boxes are separated outright. What a
    // reader gets is proximity: neighbours cluster, and that IS the structure.
    //
    // Deterministic by construction -- phyllotaxis seed positions, a fixed tick
    // count, and no randomness or wall-clock anywhere.
    let widths: Vec<f64> = entities
        .iter()
        .map(|e| {
            let eid = e.id.as_str();
            box_width(e, &|target: &str| related_pairs.contains(&(eid, target)))
        })
        .collect();
    let heights: Vec<f64> = entities.iter().map(|e| box_height(e.attributes.len())).collect();

    // Seed on a phyllotaxis spiral, scaled so the initial spread is roughly the
    // area the boxes need. Starting them all at one point would make the first
    // ticks a scramble and the result far more sensitive to tick count.
    let total_area: f64 = widths.iter().zip(&heights).map(|(w, h)| w * h).sum();
    let spread = (total_area * 2.0 / std::f64::consts::PI).sqrt().max(1.0);
    let mut px: Vec<f64> = Vec::with_capacity(n);
    let mut py: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64;
        let r = spread * (t / n as f64).sqrt();
        let a = t * GOLDEN_ANGLE;
        px.push(r * a.cos());
        py.push(r * a.sin());
    }

    // Fruchterman-Reingold. `k` is the distance the model settles at: repulsion
    // k^2/d pushes everything apart, attraction d^2/k pulls related pairs in,
    // and they balance around k.
    //
    // The temperature cap is the part that matters. WITHOUT it the layout
    // exploded -- 6742x3099 for fourteen boxes, 1.7% of the canvas covered,
    // rendering as specks once the SVG was scaled to fit. Capping each node's
    // displacement per tick, and cooling that cap toward zero, is what makes
    // the simulation settle instead of drift.
    let k = (total_area / n as f64).sqrt() + IDEAL_EDGE_GAP;

    for tick in 0..PLACEMENT_TICKS {
        let mut fx = vec![0.0_f64; n];
        let mut fy = vec![0.0_f64; n];

        // Repulsion, every pair. O(n^2) is the right call at ER scale: a
        // Barnes-Hut tree costs more to build than it saves under ~100 nodes.
        for i in 0..n {
            for j in (i + 1)..n {
                let (mut dx, mut dy) = (px[i] - px[j], py[i] - py[j]);
                let mut d = (dx * dx + dy * dy).sqrt();
                if d < 1e-6 {
                    // Coincident: separate along a deterministic axis rather
                    // than dividing by zero.
                    dx = if i % 2 == 0 { 1.0 } else { -1.0 };
                    dy = 0.0;
                    d = 1.0;
                }
                let f = k * k / d;
                let (ux, uy) = (dx / d, dy / d);
                fx[i] += ux * f;
                fy[i] += uy * f;
                fx[j] -= ux * f;
                fy[j] -= uy * f;
            }
        }

        // Attraction along relationships.
        for (i, entity) in entities.iter().enumerate() {
            for rel in &entity.relationships {
                let j = match index_of.get(rel.to.as_str()) {
                    Some(&j) if j != i => j,
                    _ => continue,
                };
                let (dx, dy) = (px[j] - px[i], py[j] - py[i]);
                let d = (dx * dx + dy * dy).sqrt();
                if d < 1e-6 {
                    continue;
                }
                let f = d * d / k;
                let (ux, uy) = (dx / d, dy / d);
                fx[i] += ux * f;
                fy[i] += uy * f;
                fx[j] -= ux * f;
                fy[j] -= uy * f;
            }
        }

        // Gravity toward the centroid. Uses the centroid rather than the origin
        // so the whole layout is never dragged across the plane just because it
        // drifted; only its spread is constrained.
        let (cx, cy) = (px.iter().sum::<f64>() / n as f64, py.iter().sum::<f64>() / n as f64);
        for i in 0..n {
            fx[i] += (cx - px[i]) * GRAVITY * k / k;
            fy[i] += (cy - py[i]) * GRAVITY;
        }

        // Cool from a bold first move down to a fine final one, and never let a
        // node travel further than the current temperature in one tick.
        let temperature = k * (1.0 - tick as f64 / PLACEMENT_TICKS as f64).powi(2) + 1.0;
        for i in 0..n {
            let d = (fx[i] * fx[i] + fy[i] * fy[i]).sqrt();
            if d < 1e-9 {
                continue;
            }
            let scale = d.min(temperature) / d;
            px[i] += fx[i] * scale;
            py[i] += fy[i] * scale;
        }

        // Hard separation. The force terms treat boxes as points, so actual
        // rectangle overlap is resolved directly -- along the cheaper axis,
        // which is what keeps the result compact rather than merely legal.
        for _ in 0..SEPARATION_PASSES {
            for i in 0..n {
                for j in (i + 1)..n {
                    let need_x = (widths[i] + widths[j]) / 2.0 + BOX_CLEARANCE;
                    let need_y = (heights[i] + heights[j]) / 2.0 + BOX_CLEARANCE;
                    let (dx, dy) = (px[j] - px[i], py[j] - py[i]);
                    let (ox, oy) = (need_x - dx.abs(), need_y - dy.abs());
                    if ox <= 0.0 || oy <= 0.0 {
                        continue; // already clear on at least one axis
                    }
                    if ox < oy {
                        let push = ox / 2.0 * if dx >= 0.0 { 1.0 } else { -1.0 };
                        px[i] -= push;
                        px[j] += push;
                    } else {
                        let push = oy / 2.0 * if dy >= 0.0 { 1.0 } else { -1.0 };
                        py[i] -= push;
                        py[j] += push;
                    }
                }
            }
        }
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

    let canvas_width =
        boxes.iter().map(|b| b.x + b.width).fold(0.0_f64, f64::max) + MARGIN;
    let canvas_height =
        boxes.iter().map(|b| b.y + b.height).fold(0.0_f64, f64::max) + MARGIN;

    let edges = route_edges(&boxes, input);

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

/// A straight run between two assigned ports, plus the label anchor.
///
/// No bends. Orthogonal routing earns its place when boxes sit on fixed tracks
/// -- a grid, or the columns this engine used to impose -- because a line then
/// has channels to follow and corners to turn. Nothing here is on a track:
/// placement is free, so the shortest path between two ports is the straight
/// one, and every bend was decoration that added length and crossings.
///
/// The port assignment is what makes this work. Ports are already spread along
/// the face pointing at the other box, so parallel lines stay separated without
/// needing a jog to hold them apart.
fn route(start: &Point, end: &Point) -> (Vec<Point>, f64, f64) {
    (
        vec![start.clone(), end.clone()],
        (start.x + end.x) / 2.0,
        (start.y + end.y) / 2.0,
    )
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

    // Which face of each box an edge leaves from and arrives at, decided by
    // where the other box actually IS. Dominant axis wins: a neighbour mostly
    // to the right is met on the right face, one mostly below on the bottom
    // face. That is only possible without columns -- when everything was in a
    // column, every line had to leave sideways however its target was placed.
    let sides: Vec<(Side, Side)> = pending
        .iter()
        .map(|&(i, j, _)| {
            let (a, b) = (&boxes[i], &boxes[j]);
            let dx = (b.x + b.width / 2.0) - (a.x + a.width / 2.0);
            let dy = (b.y + b.height / 2.0) - (a.y + a.height / 2.0);
            // Compare each axis against the boxes' own extent, so a wide pair
            // that is slightly offset vertically still meets face to face.
            let span_x = (a.width + b.width) / 2.0;
            let span_y = (a.height + b.height) / 2.0;
            let from = if (dx.abs() / span_x.max(1.0)) >= (dy.abs() / span_y.max(1.0)) {
                if dx >= 0.0 { Side::Right } else { Side::Left }
            } else if dy >= 0.0 {
                Side::Bottom
            } else {
                Side::Top
            };
            (from, from.opposite())
        })
        .collect();

    // Order the ports on each (box, side) by where the other end sits, so the
    // lines fan without crossing each other on the way out. Sorted by the other
    // box's centre with an id tie-break, so the result is deterministic.
    let mut slots: BTreeMap<(usize, Side), Vec<usize>> = BTreeMap::new();
    for (pi, &(i, j, _)) in pending.iter().enumerate() {
        slots.entry((i, sides[pi].0)).or_default().push(pi);
        slots.entry((j, sides[pi].1)).or_default().push(pi);
    }
    let mut ports: Vec<(Point, Point)> = vec![
        (Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 0.0 });
        pending.len()
    ];
    for ((owner, side), mut members) in slots {
        members.sort_by(|&a, &b| {
            let other = |pi: usize| {
                let (i, j, _) = pending[pi];
                if i == owner { j } else { i }
            };
            let (oa, ob) = (other(a), other(b));
            let (ca, cb) =
                (boxes[oa].y + boxes[oa].height / 2.0, boxes[ob].y + boxes[ob].height / 2.0);
            ca.partial_cmp(&cb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(entities[oa].id.cmp(&entities[ob].id))
        });
        let b = &boxes[owner];
        let horizontal = side.is_horizontal_face();
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
            // An edge could start and end on the same box (a self
            // relationship), so the "from" slot is only claimed once.
            let unset = ports[pi].0.x == 0.0 && ports[pi].0.y == 0.0;
            if i == owner && sides[pi].0 == side && unset {
                ports[pi].0 = point;
            } else {
                ports[pi].1 = point;
            }
        }
    }

    let mut edges = Vec::new();
    for (pi, &(i, _, rel)) in pending.iter().enumerate() {
        let (from_foot, to_foot) = feet(&rel.cardinality);
        let (points, label_x, label_y) = route(&ports[pi].0, &ports[pi].1);
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
        assert!(
            within < between,
            "widest cluster pair {within:.0} should be closer than the nearest \
             cross-cluster pair {between:.0}"
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
        let farthest_leaf = d("leaf1").max(d("leaf2"));
        for id in ["rule", "glossary", "note"] {
            assert!(
                d(id) > farthest_leaf,
                "{id} has no relationships and should sit outside the hub's leaves \
                 ({:.0} vs {farthest_leaf:.0})",
                d(id)
            );
        }
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
