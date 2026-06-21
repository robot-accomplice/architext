//! audit_model: the forbidden-artifact GATE harness for the deterministic router.
//!
//! Runs the model on every (flow, view) of a corpus and checks each §0 forbidden
//! artifact (dogleg, Z/staircase, non-orthogonal segment, unrouted edge, min-stem
//! violation, channel overlap). The routing is VALID only if the gate finds NONE;
//! having found none, the harness proceeds to validate everything else (shape
//! legality, β, crossings, length). Exits non-zero if any forbidden artifact is
//! present — so it gates CI.
//!
//! Usage: audit_model <data-dir>

use std::path::PathBuf;
use std::process;

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::precompute::model_geometry;
use architext_routing::route_model::audit::audit_routes;

/// One forbidden-artifact class: a label and the offending locations found.
struct Class {
    label: &'static str,
    hits: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: audit_model <data-dir>");
        process::exit(2);
    }
    let data_dir = PathBuf::from(&args[1]);
    let config = resolve_diagram_config_defaults();
    let geoms = match model_geometry(&data_dir, &config) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    // Six forbidden classes, aggregated across every (flow, view).
    let mut dogleg = Class { label: "dogleg (folds over itself)", hits: vec![] };
    let mut staircase = Class { label: "Z / staircase", hits: vec![] };
    let mut non_orth = Class { label: "non-orthogonal segment", hits: vec![] };
    let mut unrouted = Class { label: "unrouted edge", hits: vec![] };
    let mut short_stem = Class { label: "min-stem (bend at the wall)", hits: vec![] };
    let mut overlap = Class { label: "channel overlap (shared/coincident)", hits: vec![] };

    let (mut beta, mut crossings, mut length) = (0.0_f64, 0usize, 0.0_f64);
    let (mut n_straight, mut n_l, mut n_c, mut n_routes) = (0usize, 0usize, 0usize, 0usize);

    for g in &geoms {
        let a = audit_routes(&g.routes);
        let at = |edge: usize| format!("{} / {} edge#{}", g.flow_id, g.view_id, edge);
        let pair = |i: usize, j: usize| format!("{} / {} edges#{},{}", g.flow_id, g.view_id, i, j);
        dogleg.hits.extend(a.doglegs.iter().map(|&e| at(e)));
        staircase.hits.extend(a.staircases.iter().map(|&e| at(e)));
        non_orth.hits.extend(a.non_orthogonal.iter().map(|&e| at(e)));
        unrouted.hits.extend(a.unrouted.iter().map(|&e| at(e)));
        short_stem.hits.extend(a.short_stems.iter().map(|&e| at(e)));
        overlap.hits.extend(a.channel_overlaps.iter().map(|&(i, j)| pair(i, j)));
        beta += a.beta;
        crossings += a.crossings;
        length += a.length;
        n_straight += a.straight;
        n_l += a.ells;
        n_c += a.cees;
        n_routes += g.routes.len();
    }

    let classes = [&dogleg, &staircase, &non_orth, &unrouted, &short_stem, &overlap];
    let total_forbidden: usize = classes.iter().map(|c| c.hits.len()).sum();

    println!("=== FORBIDDEN ARTIFACT GATE (§0 hard rules) — {} ===", data_dir.display());
    println!("scanned {} (flow,view) pairs, {} routes\n", geoms.len(), n_routes);
    println!("{:<38} {:>6}", "forbidden artifact", "count");
    println!("{}", "-".repeat(46));
    for c in classes {
        println!("{:<38} {:>6}", c.label, c.hits.len());
    }
    println!("{}", "-".repeat(46));

    if total_forbidden > 0 {
        println!("\nGATE: FAIL — {total_forbidden} forbidden artifact(s); NOT proceeding to validation.\n");
        for c in classes {
            if c.hits.is_empty() {
                continue;
            }
            println!("{} ({}):", c.label, c.hits.len());
            for h in c.hits.iter().take(25) {
                println!("  - {h}");
            }
            if c.hits.len() > 25 {
                println!("  … and {} more", c.hits.len() - 25);
            }
        }
        process::exit(1);
    }

    println!("\nGATE: PASS — no forbidden artifacts.\n");
    println!("=== VALIDATION (everything else) ===");
    let legal = n_straight + n_l + n_c;
    let pct = if n_routes > 0 { 100.0 * legal as f64 / n_routes as f64 } else { 100.0 };
    println!(
        "shapes: {legal}/{n_routes} legal §0 shapes ({pct:.0}%) — straight {n_straight}, L {n_l}, C {n_c}"
    );
    if legal != n_routes {
        // unreachable while the gate passes (every non-legal shape is forbidden),
        // but assert it loudly rather than silently claim 100%.
        println!("VALIDATION: FAIL — {} route(s) are neither straight/L/C", n_routes - legal);
        process::exit(1);
    }
    println!("β total: {beta:.0}   crossings: {crossings}   length: {length:.0}");
    println!("\nVALID ✓ — gate clean, all {n_routes} routes are legal §0 shapes.");
}
