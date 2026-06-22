//! dump_structural: reproduce a structural (deployment / C4) view's plan offline
//! and print each routed edge's polyline, so the client-side WASM geometry can be
//! eyeballed without a browser.
//!
//! Usage: dump_structural <data-dir> <view-id>

use std::path::PathBuf;
use std::process;

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::plan_diagram::plan_diagram;
use architext_routing::plan_request::diagram_layout::diagram_layout_for;
use architext_routing::plan_request::{
    build_structural_plan_request, structural_relationship_count, StructuralNode,
};
use architext_routing::precompute::load_flows_and_views;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: dump_structural <data-dir> <view-id>");
        process::exit(1);
    }
    let data_dir = PathBuf::from(&args[1]);
    let view_id = &args[2];

    let (_flows, views) = load_flows_and_views(&data_dir).unwrap_or_else(|e| {
        eprintln!("load views: {e}");
        process::exit(1);
    });
    let view = views.iter().find(|v| &v.id == view_id).unwrap_or_else(|| {
        eprintln!("no view {view_id}; available: {:?}", views.iter().map(|v| &v.id).collect::<Vec<_>>());
        process::exit(1);
    });

    // Parse nodes.json into StructuralNode (id, type, dependencies).
    let nodes_text = std::fs::read_to_string(data_dir.join("nodes.json")).unwrap();
    let nodes_json: serde_json::Value = serde_json::from_str(&nodes_text).unwrap();
    // Optional isolation test: DROP_DEP="from:to" removes that one dependency edge
    // so we can see how a sibling edge re-routes without it.
    let drop = std::env::var("DROP_DEP").ok();
    let (drop_from, drop_to) = drop
        .as_deref()
        .and_then(|s| s.split_once(':'))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .unwrap_or_default();

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
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d.as_str().map(String::from))
                            .filter(|d| !(id == drop_from && *d == drop_to))
                            .collect()
                    })
                    .unwrap_or_default(),
                id,
            }
        })
        .collect();

    let config = resolve_diagram_config_defaults();
    let layout_config = config.layout.to_layout_config();
    let count = structural_relationship_count(view, &nodes);
    let layout = diagram_layout_for(view, count, Some(&layout_config));
    let req = build_structural_plan_request(view, &nodes, &layout, "orthogonal");
    let plan = plan_diagram(&req.plan_diagram_input);

    println!("canvas {:.0}x{:.0}  rels={count}", plan.canvas_width, plan.canvas_height);
    println!("--- node rects (id: x,y wxh) ---");
    for (id, r) in &plan.node_rects {
        println!("  {id}: {:.0},{:.0} {:.0}x{:.0}", r.x, r.y, r.width, r.height);
    }
    println!("--- routes (edge: point polyline) ---");
    for (id, route) in &plan.routes {
        let pts: Vec<String> = route.points.iter().map(|p| format!("({:.0},{:.0})", p.x, p.y)).collect();
        println!("  {id}: {}", pts.join(" -> "));
        if id.contains("viewer-runtime-target-data-files") {
            let mut keys: Vec<_> = route.extra.keys().collect();
            keys.sort();
            for k in keys {
                println!("      .{k} = {}", route.extra[k]);
            }
        }
    }
}
