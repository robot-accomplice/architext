// Differential test: compare js_number_to_string against Node `String(x)` over a
// large sample. Requires `node` on PATH. Skips cleanly if node is absent.
use architext_routing::js_compat::js_number_to_string;
use std::process::Command;

#[test]
fn matches_node_over_sample() {
    let mut samples: Vec<f64> = Vec::new();
    // Coordinate-like values the engine actually emits: integers and *1.6 fractions.
    for i in -2000..2000 {
        samples.push(i as f64);
        samples.push(i as f64 * 1.6);
        samples.push(i as f64 / 2.0);
    }
    let rust: Vec<String> = samples.iter().map(|x| js_number_to_string(*x)).collect();

    let input = samples.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(",");
    let script = format!("console.log(JSON.stringify([{input}].map(x=>String(x))))");
    let out = match Command::new("node").arg("-e").arg(&script).output() {
        Ok(o) if o.status.success() => o,
        _ => {
            eprintln!("node unavailable — skipping differential parity");
            return;
        }
    };
    let node: Vec<String> = serde_json::from_slice(&out.stdout).expect("node json");
    assert_eq!(rust.len(), node.len());
    for (i, (r, n)) in rust.iter().zip(node.iter()).enumerate() {
        assert_eq!(r, n, "mismatch at sample {i} ({})", samples[i]);
    }
}
