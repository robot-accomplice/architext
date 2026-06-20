//! render_model: emit a standalone SVG of one flow routed by the deterministic
//! model (route_all_slotted) — node rects + route polylines. Lets the model's
//! output be eyeballed WITHOUT touching the production plan path.
//!
//! Usage: render_model <data-dir> <flow-id> [view-id]   (SVG to stdout)

use std::path::PathBuf;
use std::process;

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::precompute::model_geometry;
use architext_routing::route_rendering::path_to_svg;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render_model <data-dir> <flow-id> [view-id]");
        process::exit(1);
    }
    let data_dir = PathBuf::from(&args[1]);
    let flow_id = &args[2];
    let view_id = args.get(3);

    let config = resolve_diagram_config_defaults();
    let geos = match model_geometry(&data_dir, &config) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    let g = match geos
        .iter()
        .find(|g| &g.flow_id == flow_id && view_id.map(|v| v == &g.view_id).unwrap_or(true))
    {
        Some(g) => g,
        None => {
            eprintln!("no geometry for flow {flow_id}; available:");
            for g in &geos {
                eprintln!("  {} / {}", g.flow_id, g.view_id);
            }
            process::exit(1);
        }
    };

    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {:.0} {:.0}\" font-family=\"monospace\">\n",
        g.canvas_width, g.canvas_height
    ));
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#0b0e11\"/>\n");
    // routes (mint), under nodes so node cards sit on top of endpoints
    for route in &g.routes {
        if route.len() < 2 {
            continue;
        }
        svg.push_str(&format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"#19f2c4\" stroke-width=\"2\"/>\n",
            path_to_svg(route)
        ));
    }
    // node cards (cyan outline) + id labels
    for (id, r) in &g.nodes {
        svg.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"4\" fill=\"#11161b\" stroke=\"#00e5ff\" stroke-width=\"1.5\"/>\n",
            r.x, r.y, r.width, r.height
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#cfe9ee\" font-size=\"11\" text-anchor=\"middle\">{}</text>\n",
            r.x + r.width / 2.0,
            r.y + r.height / 2.0 + 4.0,
            id
        ));
    }
    svg.push_str("</svg>\n");
    print!("{svg}");
}
