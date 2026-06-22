//! time_structural: time `plan_diagram` for every structural (non-flow) view in a
//! data dir, the way the browser routes them on demand. Reveals which "simple"
//! views are pathologically slow.
//!
//! Usage: time_structural <data-dir>
use std::path::PathBuf;
use std::time::Instant;

use architext_routing::plan_diagram::plan_diagram;
use architext_routing::plan_request::diagram_layout::diagram_layout_for;
use architext_routing::plan_request::{
    build_structural_plan_request, structural_relationship_count, StructuralNode,
};
use architext_routing::precompute::load_flows_and_views;
use architext_routing::diagram_config::resolve_diagram_config_defaults;

const FLOW_VIEW_TYPES: &[&str] = &["system-map", "flow-explorer", "workflow", "dataflow"];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data_dir = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| ".".into()));
    let (_flows, views) = load_flows_and_views(&data_dir).expect("load views");
    let nodes_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data_dir.join("nodes.json")).unwrap()).unwrap();

    let nodes: Vec<StructuralNode> = nodes_json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            let id = n["id"].as_str().unwrap_or_default().to_string();
            StructuralNode {
                node_type: n["type"].as_str().unwrap_or_default().to_string(),
                dependencies: n["dependencies"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|d| d.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                id,
            }
        })
        .collect();

    let config = resolve_diagram_config_defaults();
    let layout_config = config.layout.to_layout_config();

    let mut rows: Vec<(f64, String, String, usize, usize)> = Vec::new();
    for view in &views {
        // Skip flow-projection and sequence views: those route through the flow
        // path / are not node-based structural.
        if FLOW_VIEW_TYPES.contains(&view.view_type.as_str()) || view.view_type == "sequence" {
            continue;
        }
        let count = structural_relationship_count(view, &nodes);
        let layout = diagram_layout_for(view, count, Some(&layout_config));
        let req = build_structural_plan_request(view, &nodes, &layout, "orthogonal");
        let t = Instant::now();
        let plan = plan_diagram(&req.plan_diagram_input);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        rows.push((ms, view.id.clone(), view.view_type.clone(), plan.node_rects.len(), plan.routes.len()));
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("{:>9}  {:<14} {:<42} nodes edges", "ms", "type", "view");
    for (ms, id, ty, n, e) in &rows {
        println!("{ms:9.1}  {ty:<14} {id:<42} {n:>5} {e:>5}");
    }
}
