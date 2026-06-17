//! Port of `viewer/src/routing/relationshipLabels.js`.
//!
//! Structural (C4 / deployment) edges are labelled by the relationship between
//! the two nodes — not by a numbered flow step. `relationship_label(from, to)`
//! reproduces the JS precedence exactly:
//!   1. a target-id-specific phrase (the `targetSpecificLabels` table);
//!   2. a target-*type* phrase (`data-store`/`queue`/`external-service`);
//!   3. a source-*type* phrase (`actor`/`client`);
//!   4. the `"depends on"` default.
//!
//! A missing endpoint yields `"relates to"`, matching the JS `if (!from || !to)`.

/// The minimal node facts the label rule reads: the id (for the target-specific
/// table) and the C4 role `type`.
pub struct LabelNode<'a> {
    pub id: &'a str,
    pub node_type: &'a str,
}

/// Port of the JS `targetSpecificLabels` table: a `to.id` → phrase map.
fn target_specific_label(to_id: &str) -> Option<&'static str> {
    match to_id {
        "api-server" => Some("calls API"),
        "websocket-control-plane" => Some("uses websocket/control"),
        "daemon-runtime" => Some("uses runtime"),
        "unified-pipeline" => Some("runs turn"),
        "llm-service" => Some("requests model"),
        "memory-system" => Some("retrieves memory"),
        "mcp-system" => Some("uses MCP/tools"),
        "skill-plugin-system" => Some("uses skills/plugins"),
        "scheduler" => Some("schedules work"),
        "config-keystore" => Some("reads config"),
        "observability-system" => Some("records telemetry"),
        "external-channel-adapters" => Some("uses channel"),
        _ => None,
    }
}

/// Port of JS `relationshipLabel(from, to)`.
///
/// `from`/`to` are `Option` to mirror the JS guard: when either endpoint is
/// missing (an unresolved node id), the label is `"relates to"`.
pub fn relationship_label(from: Option<&LabelNode<'_>>, to: Option<&LabelNode<'_>>) -> String {
    let (from, to) = match (from, to) {
        (Some(f), Some(t)) => (f, t),
        _ => return "relates to".to_string(),
    };
    if let Some(phrase) = target_specific_label(to.id) {
        return phrase.to_string();
    }
    match to.node_type {
        "data-store" => return "reads/writes".to_string(),
        "queue" => return "publishes".to_string(),
        "external-service" => return "uses provider".to_string(),
        _ => {}
    }
    match from.node_type {
        "actor" => "uses".to_string(),
        "client" => "calls".to_string(),
        _ => "depends on".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node<'a>(id: &'a str, ty: &'a str) -> LabelNode<'a> {
        LabelNode { id, node_type: ty }
    }

    #[test]
    fn missing_endpoint_relates_to() {
        assert_eq!(relationship_label(None, Some(&node("x", "service"))), "relates to");
        assert_eq!(relationship_label(Some(&node("x", "service")), None), "relates to");
    }

    #[test]
    fn target_specific_wins_over_types() {
        // Even though `from` is an actor, the target-id phrase takes precedence.
        let f = node("maintainer", "actor");
        let t = node("api-server", "service");
        assert_eq!(relationship_label(Some(&f), Some(&t)), "calls API");
    }

    #[test]
    fn target_type_phrases() {
        let f = node("svc", "service");
        assert_eq!(relationship_label(Some(&f), Some(&node("db", "data-store"))), "reads/writes");
        assert_eq!(relationship_label(Some(&f), Some(&node("q", "queue"))), "publishes");
        assert_eq!(relationship_label(Some(&f), Some(&node("ext", "external-service"))), "uses provider");
    }

    #[test]
    fn source_type_phrases_and_default() {
        let t = node("svc", "service");
        assert_eq!(relationship_label(Some(&node("a", "actor")), Some(&t)), "uses");
        assert_eq!(relationship_label(Some(&node("c", "client")), Some(&t)), "calls");
        assert_eq!(relationship_label(Some(&node("o", "service")), Some(&t)), "depends on");
    }
}
