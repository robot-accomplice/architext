//! farm_dump: CLI binary that enumerates the plan-precompute farm for a data
//! directory and prints NDJSON `{flowId,viewId,key,hash,planJson}` per request,
//! sorted deterministically (view-order then flow-order, matching the JS oracle).
//!
//! Usage: farm_dump <data-dir>
//!
//! This binary is the Rust oracle for `viewer/tools/farm-parity-rust.mjs`.

use std::path::PathBuf;
use std::process;

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::precompute::enumerate_flow_plan_requests;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: farm_dump <data-dir>");
        process::exit(1);
    }
    let data_dir = PathBuf::from(&args[1]);
    if !data_dir.exists() {
        eprintln!("error: data dir not found: {}", data_dir.display());
        process::exit(1);
    }

    let config = resolve_diagram_config_defaults();
    let entries = match enumerate_flow_plan_requests(&data_dir, &config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    for entry in &entries {
        // Emit NDJSON — one JSON object per line.
        // Use serde_json to ensure correct escaping.
        let line = serde_json::json!({
            "flowId": entry.flow_id,
            "viewId": entry.view_id,
            "key": entry.key,
            "hash": entry.hash,
            "planJson": entry.plan_json,
        });
        println!("{}", serde_json::to_string(&line).expect("serialize ndjson line"));
    }
}
