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
//! ## Why layering rather than a grid
//!
//! A column-wrapping grid is simpler and renders acceptably for a chain of
//! one-to-many relationships. It falls apart on the shape real schemas
//! actually have: a `user` or `account` table that half the model points at.
//! Dropped in an arbitrary grid cell, a hub's edges cross most of the diagram.
//!
//! So entities are placed in layers seeded from the highest-degree entity and
//! ordered within each layer by barycenter, which is the standard remedy for
//! exactly that problem: a hub lands at the head of its component and its
//! neighbours stack in the next column, turning what a grid renders as
//! spaghetti into a fan.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
/// Nominal advance width of one character at the box's font size.
const CHAR_W: f64 = 6.6;
/// Horizontal padding inside the box, left and right.
const BOX_PAD_X: f64 = 12.0;
const BOX_MIN_W: f64 = 132.0;
const BOX_MAX_W: f64 = 268.0;
/// Horizontal gap between adjacent layers.
const COL_GAP: f64 = 88.0;
/// Vertical gap between boxes within a layer.
const ROW_GAP: f64 = 28.0;
/// Canvas margin on every side.
const MARGIN: f64 = 28.0;
/// Barycenter sweeps (down, up, down...). Two full passes settle this size of
/// graph; more sweeps stop changing the result.
const BARYCENTER_SWEEPS: usize = 4;

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

/// Estimated rendered width of an attribute row, `name  type` plus the key
/// glyph column.
fn row_text_len(attr: &ErAttributeInput) -> usize {
    let key_len = match attr.key.as_deref() {
        Some(_) => 4, // "PK  " / "FK  " / "UK  "
        None => 0,
    };
    let ref_len = attr.references.as_ref().map_or(0, |r| r.chars().count() + 3); // " > x"
    key_len + attr.name.chars().count() + 2 + attr.type_name.chars().count() + ref_len
}

fn box_width(entity: &ErEntityInput) -> f64 {
    let widest = entity
        .attributes
        .iter()
        .map(row_text_len)
        .chain(std::iter::once(entity.name.chars().count()))
        .max()
        .unwrap_or(0);
    (widest as f64 * CHAR_W + BOX_PAD_X * 2.0).clamp(BOX_MIN_W, BOX_MAX_W)
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

    // --- layer assignment ----------------------------------------------------
    // Seed each component at its highest-degree entity so a hub heads its
    // component and its neighbours fan out into the next column, rather than
    // landing in an arbitrary cell with edges crossing the diagram.
    let mut layer: Vec<usize> = vec![usize::MAX; n];
    let mut order: Vec<Vec<usize>> = Vec::new(); // order[layer] = entity indices

    let mut remaining: BTreeSet<usize> = (0..n).collect();
    while let Some(&seed) = remaining
        .iter()
        .max_by_key(|&&i| (adj[i].len(), std::cmp::Reverse(entities[i].id.as_str())))
    {
        let mut queue = VecDeque::new();
        queue.push_back((seed, 0usize));
        layer[seed] = 0;
        remaining.remove(&seed);

        while let Some((i, depth)) = queue.pop_front() {
            if order.len() <= depth {
                order.resize(depth + 1, Vec::new());
            }
            order[depth].push(i);
            for &j in &adj[i] {
                if remaining.remove(&j) {
                    layer[j] = depth + 1;
                    queue.push_back((j, depth + 1));
                }
            }
        }
    }

    // --- barycenter ordering within each layer -------------------------------
    for sweep in 0..BARYCENTER_SWEEPS {
        let downward = sweep % 2 == 0;
        let layer_indices: Vec<usize> = if downward {
            (1..order.len()).collect()
        } else {
            (0..order.len().saturating_sub(1)).rev().collect()
        };
        for li in layer_indices {
            let neighbour_layer = if downward { li - 1 } else { li + 1 };
            // position within the reference layer, by entity index
            let mut pos: BTreeMap<usize, f64> = BTreeMap::new();
            for (p, &idx) in order[neighbour_layer].iter().enumerate() {
                pos.insert(idx, p as f64);
            }
            let mut scored: Vec<(f64, &str, usize)> = order[li]
                .iter()
                .enumerate()
                .map(|(current, &idx)| {
                    let vals: Vec<f64> =
                        adj[idx].iter().filter_map(|j| pos.get(j).copied()).collect();
                    let bary = if vals.is_empty() {
                        // No anchor in the reference layer: hold position, so a
                        // disconnected entity does not drift to the top on
                        // every sweep and make the layout unstable.
                        current as f64
                    } else {
                        vals.iter().sum::<f64>() / vals.len() as f64
                    };
                    (bary, entities[idx].id.as_str(), idx)
                })
                .collect();
            // Explicit tie-break on id: `sort_by` alone would leave equal
            // barycenters in whatever order the previous sweep produced, which
            // is stable here but not obviously so. Being explicit makes
            // determinism a property of the code, not of an invariant a reader
            // has to reconstruct.
            scored.sort_by(|a, b| {
                a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(b.1))
            });
            order[li] = scored.into_iter().map(|(_, _, idx)| idx).collect();
        }
    }

    // --- geometry ------------------------------------------------------------
    let widths: Vec<f64> = entities.iter().map(box_width).collect();
    let heights: Vec<f64> = entities.iter().map(|e| box_height(e.attributes.len())).collect();

    let layer_widths: Vec<f64> = order
        .iter()
        .map(|l| l.iter().map(|&i| widths[i]).fold(0.0_f64, f64::max))
        .collect();
    let layer_heights: Vec<f64> = order
        .iter()
        .map(|l| {
            let stack: f64 = l.iter().map(|&i| heights[i]).sum();
            stack + ROW_GAP * (l.len().saturating_sub(1)) as f64
        })
        .collect();
    let tallest = layer_heights.iter().copied().fold(0.0_f64, f64::max);

    let mut layer_x = Vec::with_capacity(order.len());
    let mut x = MARGIN;
    for w in &layer_widths {
        layer_x.push(x);
        x += w + COL_GAP;
    }
    let canvas_width = x - COL_GAP + MARGIN;
    let canvas_height = tallest + MARGIN * 2.0;

    let mut boxes: Vec<Option<ErBox>> = vec![None; n];
    for (li, entity_indices) in order.iter().enumerate() {
        // Centre each layer vertically so short columns sit beside tall ones.
        let mut y = MARGIN + (tallest - layer_heights[li]) / 2.0;
        for &i in entity_indices {
            let entity = &entities[i];
            let declared: BTreeSet<&str> =
                entity.relationships.iter().map(|r| r.to.as_str()).collect();
            boxes[i] = Some(ErBox {
                id: entity.id.clone(),
                name: entity.name.clone(),
                // Centre boxes narrower than their layer.
                x: layer_x[li] + (layer_widths[li] - widths[i]) / 2.0,
                y,
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
                        relationship_declared: a
                            .references
                            .as_deref()
                            .is_none_or(|t| declared.contains(t)),
                    })
                    .collect(),
            });
            y += heights[i] + ROW_GAP;
        }
    }
    let boxes: Vec<ErBox> = boxes.into_iter().map(|b| b.expect("every entity placed")).collect();

    // --- edges ---------------------------------------------------------------
    let mut edges = Vec::new();
    for (i, entity) in entities.iter().enumerate() {
        for rel in &entity.relationships {
            let j = match index_of.get(rel.to.as_str()) {
                Some(&j) => j,
                None => continue, // dangling; validation already rejected it
            };
            let (from_foot, to_foot) = feet(&rel.cardinality);
            let (points, label_x, label_y) = route(&boxes[i], &boxes[j], layer[i], layer[j]);
            edges.push(ErEdge {
                from: entity.id.clone(),
                to: rel.to.clone(),
                label: rel.label.clone(),
                cardinality: rel.cardinality.clone(),
                points,
                from_foot,
                to_foot,
                label_x,
                label_y,
            });
        }
    }

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

/// Orthogonal route between two boxes, plus the label anchor.
///
/// Layering makes most edges go left-to-right between adjacent columns, so the
/// common case is a clean three-segment path out of one box's side and into the
/// other's. Same-layer edges leave and enter vertically instead, so they do not
/// run along the column and through the boxes between them.
fn route(a: &ErBox, b: &ErBox, layer_a: usize, layer_b: usize) -> (Vec<Point>, f64, f64) {
    let a_mid_y = a.y + a.height / 2.0;
    let b_mid_y = b.y + b.height / 2.0;

    if layer_a == layer_b {
        // Same column: exit the bottom of the upper box, enter the top of the
        // lower one, jogging clear of the column on the right.
        let (top, bottom) = if a.y <= b.y { (a, b) } else { (b, a) };
        let jog = top.x + top.width.max(bottom.width) + COL_GAP / 3.0;
        let start = Point { x: top.x + top.width, y: top.y + top.height / 2.0 };
        let end = Point { x: bottom.x + bottom.width, y: bottom.y + bottom.height / 2.0 };
        let label_y = (start.y + end.y) / 2.0;
        let points = vec![
            Point { x: start.x, y: start.y },
            Point { x: jog, y: start.y },
            Point { x: jog, y: end.y },
            end,
        ];
        return (points, jog, label_y);
    }

    // Different columns: leave the right side of the left box, enter the left
    // side of the right box.
    let (left, right, left_y, right_y) =
        if layer_a < layer_b { (a, b, a_mid_y, b_mid_y) } else { (b, a, b_mid_y, a_mid_y) };
    let start = Point { x: left.x + left.width, y: left_y };
    let end = Point { x: right.x, y: right_y };
    let mid_x = (start.x + end.x) / 2.0;
    let points = if (start.y - end.y).abs() < f64::EPSILON {
        vec![start.clone(), end.clone()]
    } else {
        vec![
            start.clone(),
            Point { x: mid_x, y: start.y },
            Point { x: mid_x, y: end.y },
            end.clone(),
        ]
    };
    (points, mid_x, (start.y + end.y) / 2.0)
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

    #[test]
    fn hub_neighbours_land_in_one_column_beside_the_hub() {
        // WHY: this is the concrete quality claim the layering exists to make.
        // A grid would scatter these five across rows and columns and cross the
        // hub's edges over each other. Here the hub heads its component and its
        // five neighbours stack in the next column, which is a fan, not
        // spaghetti.
        let plan = plan_er(&hub_input());
        let hub = plan.boxes.iter().find(|b| b.id == "account").unwrap();
        let others: Vec<&ErBox> = plan.boxes.iter().filter(|b| b.id != "account").collect();
        assert_eq!(others.len(), 5);
        let col_x = others[0].x;
        for o in &others {
            assert_eq!(o.x, col_x, "{} should share the neighbour column", o.id);
            assert!(o.x > hub.x, "{} should sit to the right of the hub", o.id);
        }
    }

    /// Two hubs sharing the same three children, declared in OPPOSITE orders.
    ///
    /// The single-hub fan cannot test crossing-minimisation at all: every edge
    /// shares the hub as an endpoint, so no pair is even eligible to cross.
    /// This shape is the smallest one where ordering decides the outcome --
    /// with the children in the wrong order the two hubs' edges cross, and with
    /// them in the right order they do not.
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
                },
            ],
            canvas_width: 100.0,
            canvas_height: 100.0,
        };
        assert_eq!(count_crossings(&plan), 1, "an X must count as one crossing");
    }

    #[test]
    fn shared_children_are_ordered_without_crossings() {
        // The fitness assertion. Deliberately NOT folded into the routing
        // corpus ratchet: crossing-minimisation between variable-height boxes
        // is a different quality metric than lane-based flow routing, and one
        // number cannot answer both questions.
        let plan = plan_er(&shared_children_input());
        let crossings = count_crossings(&plan);
        assert_eq!(
            crossings, 0,
            "barycenter ordering should resolve two hubs over shared children; \
             got {crossings} crossings"
        );
    }

    #[test]
    fn empty_input_is_a_valid_empty_plan() {
        let plan = plan_er(&ErInput::default());
        assert!(plan.boxes.is_empty() && plan.edges.is_empty());
        assert!(plan.canvas_width > 0.0 && plan.canvas_height > 0.0);
    }

    // --- crossing counter (test-only fitness helper) ------------------------

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
