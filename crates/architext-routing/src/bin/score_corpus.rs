//! score_corpus: run every (flow, view) plan through the REAL plan path with
//! diagnostics on, and print the deterministic-model score
//! S = (β bend-score, crossings, length) per pair plus totals.
//!
//! This is the score-baseline harness for ROUTING_DETERMINISTIC_MODEL.md §5 — it
//! works on ANY corpus (routing-corpus AND FlowForge) because it drives the full
//! plan_diagram path the viewer/farm use, not the minimal corpus-fitness
//! reimplementation. β counts a clean bend as its count and a reversal (dogleg)
//! as REVERSAL_BEND_PENALTY (99), so any dogleg dominates the score.
//!
//! Usage: score_corpus <data-dir>

use std::path::PathBuf;
use std::process;

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::precompute::score_flow_plans;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: score_corpus <data-dir>");
        process::exit(1);
    }
    let data_dir = PathBuf::from(&args[1]);
    if !data_dir.exists() {
        eprintln!("error: data dir not found: {}", data_dir.display());
        process::exit(1);
    }

    let config = resolve_diagram_config_defaults();
    let scored = match score_flow_plans(&data_dir, &config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    println!("{:<46} {:>7} {:>5} {:>9}", "flow / view", "β", "X", "length");
    println!("{}", "-".repeat(72));
    let (mut tb, mut tx, mut tl, mut td) = (0.0_f64, 0_usize, 0.0_f64, 0_usize);
    for (flow, view, m) in &scored {
        tb += m.bend_score;
        tx += m.crossings;
        tl += m.total_length;
        td += m.doglegs;
        let flag = if m.doglegs > 0 { "  <-- dogleg" } else { "" };
        println!(
            "{:<46} {:>7.0} {:>5} {:>9.0}{}",
            format!("{flow} / {view}"),
            m.bend_score,
            m.crossings,
            m.total_length,
            flag
        );
    }
    println!("{}", "-".repeat(72));
    println!(
        "TOTAL S = (β {:.0}, crossings {}, length {:.0})    [raw doglegs: {}]",
        tb, tx, tl, td
    );
    println!("pairs scored: {}", scored.len());
}
