//! score_model: head-to-head of the current engine vs the deterministic model
//! (route_all) on every (flow, view) of a corpus, scored by S = (β, crossings,
//! length). ROUTING_DETERMINISTIC_MODEL.md §5 step 2-3.
//!
//! Usage: score_model <data-dir>

use std::path::PathBuf;
use std::process;

use architext_routing::diagram_config::resolve_diagram_config_defaults;
use architext_routing::precompute::score_model_vs_engine;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: score_model <data-dir>");
        process::exit(1);
    }
    let data_dir = PathBuf::from(&args[1]);
    let config = resolve_diagram_config_defaults();
    let pairs = match score_model_vs_engine(&data_dir, &config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    println!(
        "{:<44} {:>10} {:>10}   {:>8} {:>8}",
        "flow / view", "β engine", "β model", "X eng", "X model"
    );
    println!("{}", "-".repeat(90));
    let (mut eb, mut mb, mut ex, mut mx) = (0.0_f64, 0.0_f64, 0usize, 0usize);
    let mut unrouted = 0usize;
    for p in &pairs {
        eb += p.engine.bend_score;
        mb += p.model_beta;
        ex += p.engine.crossings;
        mx += p.model_crossings;
        unrouted += p.model_unrouted;
        let win = if p.model_beta < p.engine.bend_score {
            " <= β improved"
        } else if p.model_beta > p.engine.bend_score {
            " !! β regressed"
        } else {
            ""
        };
        let flag = if p.model_unrouted > 0 {
            format!("  [unrouted {}]", p.model_unrouted)
        } else {
            String::new()
        };
        println!(
            "{:<44} {:>10.0} {:>10.0}   {:>8} {:>8}{}{}",
            format!("{} / {}", p.flow_id, p.view_id),
            p.engine.bend_score,
            p.model_beta,
            p.engine.crossings,
            p.model_crossings,
            win,
            flag
        );
    }
    println!("{}", "-".repeat(90));
    println!(
        "TOTAL  β: engine {:.0}  ->  model {:.0}    |    crossings: engine {}  ->  model {}",
        eb, mb, ex, mx
    );
    if unrouted > 0 {
        println!("WARNING: {unrouted} edge(s) the model could not route (monotone detour impossible with side-centre mounts)");
    }
    println!(
        "β verdict: model is {} on bend-score",
        if mb < eb {
            "BETTER"
        } else if mb > eb {
            "WORSE"
        } else {
            "EQUAL"
        }
    );
}
