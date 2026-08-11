//! Pure view-model for Data Model mode.
//!
//! Everything here is free of Leptos and web-sys so it tests natively, the same
//! split `code_graph_view_model` uses. The component in
//! `components::data_model_panel` renders what these functions return.

use architext_routing::plan_er::{
    ErAttributeInput, ErEntityInput, ErInput, ErRelationshipInput, ErRow,
};

use crate::data::models::EntitiesDoc;

/// The framings Data Model offers.
///
/// Slice 1 renders `Schema`. The other three ship VISIBLY DISABLED rather than
/// hidden, so the mode is honest about what is coming instead of implying it is
/// finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    Schema,
    Ownership,
    Sensitivity,
    Usage,
}

impl Projection {
    pub const ALL: [Projection; 4] =
        [Projection::Schema, Projection::Ownership, Projection::Sensitivity, Projection::Usage];

    pub fn label(self) -> &'static str {
        match self {
            Projection::Schema => "Schema",
            Projection::Ownership => "Ownership",
            Projection::Sensitivity => "Sensitivity",
            Projection::Usage => "Usage",
        }
    }

    /// Whether slice 1 can render this projection.
    pub fn available(self) -> bool {
        matches!(self, Projection::Schema)
    }

    /// Why a projection is not selectable yet, shown as its tooltip. A disabled
    /// control with no explanation reads as a bug rather than a roadmap.
    pub fn unavailable_reason(self) -> Option<&'static str> {
        match self {
            Projection::Schema => None,
            Projection::Ownership => Some("Ownership tints arrive in a later slice."),
            Projection::Sensitivity => Some("Sensitivity tints arrive in a later slice."),
            Projection::Usage => {
                Some("Usage needs reads/writes on flow steps, which is a later slice.")
            }
        }
    }
}

/// Adapt the fetched document to the layout engine's input.
pub fn to_er_input(doc: &EntitiesDoc) -> ErInput {
    ErInput {
        entities: doc
            .entities
            .iter()
            .map(|e| ErEntityInput {
                id: e.id.clone(),
                name: e.name.clone(),
                summary: e.summary.clone(),
                owner_node_id: e.owner_node_id.clone(),
                data_class_ids: e.data_class_ids.clone(),
                attributes: e
                    .attributes
                    .iter()
                    .map(|a| ErAttributeInput {
                        name: a.name.clone(),
                        type_name: a.type_name.clone(),
                        key: a.key.clone(),
                        required: a.required,
                        references: a.references.clone(),
                    })
                    .collect(),
                relationships: e
                    .relationships
                    .iter()
                    .map(|r| ErRelationshipInput {
                        to: r.to.clone(),
                        cardinality: r.cardinality.clone(),
                        label: r.label.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The short glyph shown in an attribute row's key column.
pub fn key_glyph(key: Option<&str>) -> &'static str {
    match key {
        Some("primary") => "PK",
        Some("foreign") => "FK",
        Some("unique") => "UK",
        _ => "",
    }
}

/// What an attribute row says about the entity its foreign key names.
///
/// `relationships` is the sole source of rendered edges and `references` only
/// annotates the column, so a foreign key with no matching relationship draws
/// NOTHING. That is legitimate -- an author may not want the edge -- but from
/// the diagram it is indistinguishable from a mistake. The validator cannot
/// help: `ValidationOutcome` carries errors and nothing else, promoting this to
/// an error would be wrong, and adding a warning channel would change the
/// validator's public shape, the CLI output, `/api/status` and doctor. So the
/// annotation lives here, where the reader is already looking at the gap.
pub fn fk_annotation(row: &ErRow) -> Option<String> {
    let target = row.references.as_ref()?;
    Some(if row.relationship_declared {
        target.clone()
    } else {
        format!("{target} (no relationship declared)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{Entity, EntityAttribute, EntityRelationship};

    fn doc() -> EntitiesDoc {
        EntitiesDoc {
            entities: vec![Entity {
                id: "release".into(),
                name: "Release".into(),
                summary: None,
                owner_node_id: None,
                data_class_ids: vec![],
                attributes: vec![
                    EntityAttribute {
                        name: "id".into(),
                        type_name: "uuid".into(),
                        key: Some("primary".into()),
                        required: false,
                        references: None,
                    },
                    EntityAttribute {
                        name: "item_id".into(),
                        type_name: "uuid".into(),
                        key: Some("foreign".into()),
                        required: false,
                        references: Some("item".into()),
                    },
                ],
                relationships: vec![EntityRelationship {
                    to: "item".into(),
                    cardinality: "one-to-many".into(),
                    label: None,
                }],
            }],
        }
    }

    #[test]
    fn entities_convert_to_layout_input_without_loss() {
        let input = to_er_input(&doc());
        assert_eq!(input.entities.len(), 1);
        assert_eq!(input.entities[0].attributes.len(), 2);
        assert_eq!(input.entities[0].relationships.len(), 1);
        assert_eq!(input.entities[0].attributes[1].references.as_deref(), Some("item"));
        assert_eq!(input.entities[0].attributes[0].key.as_deref(), Some("primary"));
    }

    #[test]
    fn an_unreferenced_foreign_key_is_annotated_for_the_reader() {
        // WHY: this is the one place the design deliberately leaves a gap in the
        // diagram (a foreign key that draws no edge). If the annotation stops
        // distinguishing the two cases, that gap becomes silent and the reader
        // is left deciding whether a missing line is a bug.
        let with_rel = architext_routing::plan_er::plan_er(&to_er_input(&doc()));
        let row = &with_rel.boxes.iter().find(|b| b.id == "release").unwrap().rows[1];
        assert_eq!(fk_annotation(row).as_deref(), Some("item"));

        let mut d = doc();
        d.entities[0].relationships.clear();
        let without = architext_routing::plan_er::plan_er(&to_er_input(&d));
        let row = &without.boxes.iter().find(|b| b.id == "release").unwrap().rows[1];
        assert_eq!(fk_annotation(row).as_deref(), Some("item (no relationship declared)"));
    }

    #[test]
    fn a_non_foreign_attribute_gets_no_annotation() {
        let plan = architext_routing::plan_er::plan_er(&to_er_input(&doc()));
        let row = &plan.boxes.iter().find(|b| b.id == "release").unwrap().rows[0];
        assert!(fk_annotation(row).is_none());
    }

    #[test]
    fn only_schema_is_available_and_the_rest_explain_themselves() {
        // WHY: "visibly disabled rather than hidden" is a spec decision. A
        // disabled control with no reason reads as broken, so every unavailable
        // projection must carry an explanation.
        assert!(Projection::Schema.available());
        for p in Projection::ALL.iter().filter(|p| !p.available()) {
            assert!(
                p.unavailable_reason().is_some(),
                "{} is disabled and must say why",
                p.label()
            );
        }
        assert!(Projection::Schema.unavailable_reason().is_none());
    }

    #[test]
    fn key_glyphs_cover_the_enumerated_keys() {
        assert_eq!(key_glyph(Some("primary")), "PK");
        assert_eq!(key_glyph(Some("foreign")), "FK");
        assert_eq!(key_glyph(Some("unique")), "UK");
        assert_eq!(key_glyph(None), "");
    }
}
