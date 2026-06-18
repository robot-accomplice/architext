//! Pure Blast Radius compute: everything a single architecture node reaches.
//!
//! Faithful port of `viewer/src/presentation/blastRadius.js` `blastRadiusForNode`:
//! for a focused node it gathers the files it owns (`sourcePaths` ownership),
//! what it depends on (forward `dependencies`), what depends on it (reverse
//! edges), and the flows / views / data classes / decisions / risks it
//! participates in. Participation is the UNION of the node's authored
//! cross-references (`relatedFlows` / `relatedDecisions` / `knownRisks` /
//! `dataHandled`) and the derived links (flow steps naming the node; decisions /
//! risks whose `relatedNodes` name it; views whose lanes include it) — exactly
//! the JS union.
//!
//! Leptos-free and native-testable. Owner resolution reuses the single source
//! `repo_tree_model::{build_owner_index, resolve_owner}`. Color is NOT decided
//! here — the panel single-sources it from `diagram::role_color_var` (node type)
//! and the `--sev-*` / `--sens-*` scales (severity / sensitivity).

use crate::data::models::{DataClass, Decision, Flow, Node, Risk, View};
use crate::repo_tree_model::{build_owner_index, resolve_owner, FileEntry};

/// A reference to a node (the focus node, or a dependency/dependent). `name`
/// falls back to the id, `node_type` to `"unknown"` for a dangling id — faithful
/// to the JS `nodeRef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRef {
    pub id: String,
    pub name: String,
    pub node_type: String,
}

/// A repo file the focused node owns (longest-prefix `sourcePaths` match).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFile {
    pub path: String,
    pub size: Option<u64>,
}

/// A flow / view the node appears in (id + display name; view also carries type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewRef {
    pub id: String,
    pub name: String,
    pub view_type: String,
}

/// A data class the node handles (id + name + sensitivity for the `--sens-*` chip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRef {
    pub id: String,
    pub name: String,
    pub sensitivity: Option<String>,
}

/// A risk referencing the node (id + title + severity for the `--sev-*` badge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskRef {
    pub id: String,
    pub title: String,
    pub severity: Option<String>,
}

/// A decision referencing the node (id + title).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRef {
    pub id: String,
    pub title: String,
}

/// The full reach of one focused node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastRadius {
    pub node: NodeRef,
    pub owned_files: Vec<OwnedFile>,
    pub declared_paths: Vec<String>,
    pub depends_on: Vec<NodeRef>,
    pub dependents: Vec<NodeRef>,
    pub flows: Vec<NamedRef>,
    pub decisions: Vec<DecisionRef>,
    pub risks: Vec<RiskRef>,
    pub data_handled: Vec<DataRef>,
    pub views: Vec<ViewRef>,
}

impl BlastRadius {
    /// Total elements reached, across every section (the header "reach" count).
    /// Mirrors the JS `reachCount` sum.
    pub fn reach_count(&self) -> usize {
        self.depends_on.len()
            + self.dependents.len()
            + self.flows.len()
            + self.decisions.len()
            + self.risks.len()
            + self.data_handled.len()
            + self.views.len()
            + self.owned_files.len()
    }
}

/// Resolve a node id to a `NodeRef`, falling back to id/"unknown" for a dangling
/// id (JS `nodeRef`). Index is `id -> position in nodes`.
fn node_ref(id: &str, nodes: &[Node], index: &std::collections::HashMap<&str, usize>) -> NodeRef {
    match index.get(id).and_then(|&i| nodes.get(i)) {
        Some(n) => NodeRef { id: id.to_string(), name: n.name.clone(), node_type: n.node_type.clone() },
        None => NodeRef { id: id.to_string(), name: id.to_string(), node_type: "unknown".to_string() },
    }
}

/// Push `id` onto `out` if it is present in `present` and not already pushed.
/// Preserves first-seen order (JS `Set` insertion order) — declared ids first,
/// then derived ids.
fn push_unique(out: &mut Vec<String>, seen: &mut std::collections::HashSet<String>, id: &str) {
    if seen.insert(id.to_string()) {
        out.push(id.to_string());
    }
}

/// The dataset slices the blast-radius compute reads, borrowed together so the
/// entry point stays a clean two-argument call (focus + inputs). `files` is the
/// live repo file list (for owned files); pass an empty slice when it has not
/// been fetched yet.
pub struct BlastInputs<'a> {
    pub nodes: &'a [Node],
    pub flows: &'a [Flow],
    pub decisions: &'a [Decision],
    pub risks: &'a [Risk],
    pub data_classes: &'a [DataClass],
    pub views: &'a [View],
    pub files: &'a [FileEntry],
}

/// Compute the blast radius for `focus_id`. Returns `None` if the id is unknown
/// (JS returns `null`).
pub fn blast_radius_for_node(focus_id: &str, input: &BlastInputs<'_>) -> Option<BlastRadius> {
    let BlastInputs { nodes, flows, decisions, risks, data_classes, views, files } = *input;
    let index: std::collections::HashMap<&str, usize> =
        nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let focus = nodes.get(*index.get(focus_id)?)?;

    // Owned files: every file whose longest-prefix owner is the focus node.
    let owner_index = build_owner_index(nodes);
    let owned_files = files
        .iter()
        .filter(|f| !f.path.is_empty())
        .filter(|f| resolve_owner(&f.path, &owner_index).map(|i| nodes[i].id.as_str()) == Some(focus_id))
        .map(|f| OwnedFile { path: f.path.clone(), size: f.size })
        .collect();

    // Forward edges (focus -> X) restricted to known nodes; reverse edges (X -> focus).
    let depends_on = focus
        .dependencies
        .iter()
        .filter(|id| index.contains_key(id.as_str()))
        .map(|id| node_ref(id, nodes, &index))
        .collect();
    let dependents = nodes
        .iter()
        .filter(|n| n.dependencies.iter().any(|d| d == focus_id))
        .map(|n| node_ref(&n.id, nodes, &index))
        .collect();

    // Flows: declared `relatedFlows` ∪ flows with a step from/to the focus node.
    let mut flow_ids = Vec::new();
    let mut flow_seen = std::collections::HashSet::new();
    let flows_have: std::collections::HashSet<&str> = flows.iter().map(|f| f.id.as_str()).collect();
    for id in &focus.related_flows {
        if flows_have.contains(id.as_str()) {
            push_unique(&mut flow_ids, &mut flow_seen, id);
        }
    }
    for f in flows {
        if f.steps.iter().any(|s| s.from == focus_id || s.to == focus_id) {
            push_unique(&mut flow_ids, &mut flow_seen, &f.id);
        }
    }
    let related_flows = flow_ids
        .iter()
        .map(|id| {
            let name = flows.iter().find(|f| &f.id == id).map(|f| f.name.clone()).unwrap_or_else(|| id.clone());
            NamedRef { id: id.clone(), name }
        })
        .collect();

    // Decisions: declared `relatedDecisions` ∪ decisions whose `relatedNodes` name the focus.
    let mut dec_ids = Vec::new();
    let mut dec_seen = std::collections::HashSet::new();
    let dec_have: std::collections::HashSet<&str> = decisions.iter().map(|d| d.id.as_str()).collect();
    for id in &focus.related_decisions {
        if dec_have.contains(id.as_str()) {
            push_unique(&mut dec_ids, &mut dec_seen, id);
        }
    }
    for d in decisions {
        if d.related_nodes.iter().any(|n| n == focus_id) {
            push_unique(&mut dec_ids, &mut dec_seen, &d.id);
        }
    }
    let related_decisions = dec_ids
        .iter()
        .map(|id| {
            let title = decisions.iter().find(|d| &d.id == id).map(|d| d.title.clone()).unwrap_or_else(|| id.clone());
            DecisionRef { id: id.clone(), title }
        })
        .collect();

    // Risks: declared `knownRisks` ∪ risks whose `relatedNodes` name the focus.
    let mut risk_ids = Vec::new();
    let mut risk_seen = std::collections::HashSet::new();
    let risk_have: std::collections::HashSet<&str> = risks.iter().map(|r| r.id.as_str()).collect();
    for id in &focus.known_risks {
        if risk_have.contains(id.as_str()) {
            push_unique(&mut risk_ids, &mut risk_seen, id);
        }
    }
    for r in risks {
        if r.related_nodes.iter().any(|n| n == focus_id) {
            push_unique(&mut risk_ids, &mut risk_seen, &r.id);
        }
    }
    let related_risks = risk_ids
        .iter()
        .map(|id| {
            let r = risks.iter().find(|r| &r.id == id);
            RiskRef {
                id: id.clone(),
                title: r.map(|r| r.title.clone()).unwrap_or_else(|| id.clone()),
                severity: r.and_then(|r| r.severity.clone()),
            }
        })
        .collect();

    // Data handled: declared `dataHandled` filtered to known data classes (JS keeps order).
    let dc_have: std::collections::HashMap<&str, &DataClass> =
        data_classes.iter().map(|d| (d.id.as_str(), d)).collect();
    let data_handled = focus
        .data_handled
        .iter()
        .filter_map(|id| {
            dc_have.get(id.as_str()).map(|d| DataRef {
                id: id.clone(),
                name: d.name.clone(),
                sensitivity: d.sensitivity.clone(),
            })
        })
        .collect();

    // Views: every view with a lane that includes the focus node.
    let appears_in_views = views
        .iter()
        .filter(|v| v.lanes.iter().any(|l| l.node_ids.iter().any(|n| n == focus_id)))
        .map(|v| ViewRef { id: v.id.clone(), name: v.name.clone(), view_type: v.view_type.clone() })
        .collect();

    Some(BlastRadius {
        node: node_ref(focus_id, nodes, &index),
        owned_files,
        declared_paths: focus.source_paths.clone(),
        depends_on,
        dependents,
        flows: related_flows,
        decisions: related_decisions,
        risks: related_risks,
        data_handled,
        views: appears_in_views,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::models::{DataClass, Decision, Flow, FlowStep, Lane, Node, Risk, View};

    fn node(id: &str, ty: &str, deps: &[&str]) -> Node {
        Node {
            id: id.to_string(),
            node_type: ty.to_string(),
            name: format!("{id} name"),
            summary: None,
            owner: None,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            source_paths: Vec::new(),
            related_flows: Vec::new(),
            related_decisions: Vec::new(),
            known_risks: Vec::new(),
            data_handled: Vec::new(),
        }
    }

    fn step(id: &str, from: &str, to: &str) -> FlowStep {
        FlowStep {
            id: id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            action: "act".to_string(),
            summary: None,
            kind: None,
            outcome: None,
            return_of: None,
        }
    }

    #[test]
    fn unknown_focus_returns_none() {
        let nodes = vec![node("a", "service", &[])];
        let input = BlastInputs {
            nodes: &nodes, flows: &[], decisions: &[], risks: &[],
            data_classes: &[], views: &[], files: &[],
        };
        assert!(blast_radius_for_node("missing", &input).is_none());
    }

    #[test]
    fn reach_unions_declared_and_derived_links() {
        // a depends on b; c depends on a (reverse). a declares relatedFlows=[f1];
        // f2 has a step touching a (derived). decision d names a in relatedNodes;
        // a declares knownRisks=[r1]; a handles data dc1; a appears in view v1.
        let mut a = node("a", "service", &["b"]);
        a.source_paths = vec!["src/a".into()];
        a.related_flows = vec!["f1".into()];
        a.known_risks = vec!["r1".into()];
        a.data_handled = vec!["dc1".into(), "missing-dc".into()];
        let nodes = vec![a, node("b", "data", &[]), node("c", "client", &["a"])];

        let flows = vec![
            Flow { id: "f1".into(), name: "Flow One".into(), status: None, summary: None, trigger: None, steps: vec![], sequence_frames: vec![] },
            Flow { id: "f2".into(), name: "Flow Two".into(), status: None, summary: None, trigger: None, steps: vec![step("s1", "x", "a")], sequence_frames: vec![] },
            Flow { id: "f3".into(), name: "Flow Three".into(), status: None, summary: None, trigger: None, steps: vec![step("s2", "x", "y")], sequence_frames: vec![] },
        ];
        let decisions = vec![Decision {
            id: "d".into(), title: "Decision D".into(), status: None, context: None,
            decision: None, related_nodes: vec!["a".into()],
        }];
        let risks = vec![Risk {
            id: "r1".into(), title: "Risk One".into(), category: None, severity: Some("high".into()),
            status: None, summary: None, related_nodes: vec![],
        }];
        let data_classes = vec![DataClass {
            id: "dc1".into(), name: "PII".into(), sensitivity: Some("high".into()), handling: None,
        }];
        let views = vec![View {
            id: "v1".into(), name: "Map".into(), view_type: "system-map".into(), summary: None,
            scope_node_id: None,
            lanes: vec![Lane { id: "l".into(), name: None, node_ids: vec!["a".into(), "b".into()] }],
        }];
        let files = vec![
            FileEntry { path: "src/a/main.rs".into(), size: Some(10), mtime: None },
            FileEntry { path: "src/other/x.rs".into(), size: Some(5), mtime: None },
        ];

        let input = BlastInputs {
            nodes: &nodes, flows: &flows, decisions: &decisions, risks: &risks,
            data_classes: &data_classes, views: &views, files: &files,
        };
        let r = blast_radius_for_node("a", &input).expect("focus a is known");

        assert_eq!(r.node.name, "a name");
        assert_eq!(r.node.node_type, "service");
        // depends_on = [b]; dependents = [c].
        assert_eq!(r.depends_on.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), vec!["b"]);
        assert_eq!(r.dependents.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), vec!["c"]);
        // flows = declared f1 then derived f2 (f3 untouched). Declared-first order.
        assert_eq!(r.flows.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["f1", "f2"]);
        assert_eq!(r.flows[0].name, "Flow One");
        // decision via relatedNodes.
        assert_eq!(r.decisions.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), vec!["d"]);
        // risk via declared knownRisks, severity carried.
        assert_eq!(r.risks.len(), 1);
        assert_eq!(r.risks[0].severity.as_deref(), Some("high"));
        // data: dc1 kept, missing-dc dropped (unknown class).
        assert_eq!(r.data_handled.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), vec!["dc1"]);
        assert_eq!(r.data_handled[0].sensitivity.as_deref(), Some("high"));
        // views: v1 (a is in a lane).
        assert_eq!(r.views.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), vec!["v1"]);
        // owned files: only src/a/main.rs (resolves to a via src/a prefix).
        assert_eq!(r.owned_files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["src/a/main.rs"]);
        // reach = 1+1+2+1+1+1+1+1 = 9.
        assert_eq!(r.reach_count(), 9);
    }

    #[test]
    fn dangling_dependency_falls_back_to_unknown_type() {
        // a depends on a node not present → filtered out of depends_on (JS filters
        // forward edges to known ids), so depends_on is empty.
        let nodes = vec![node("a", "service", &["ghost"])];
        let input = BlastInputs {
            nodes: &nodes, flows: &[], decisions: &[], risks: &[],
            data_classes: &[], views: &[], files: &[],
        };
        let r = blast_radius_for_node("a", &input).unwrap();
        assert!(r.depends_on.is_empty());
    }
}
