//! `explain [topic]` — port of `explainTopic` in
//! `src/adapters/cli/architext-cli.mjs` (~line 1051).

use std::process;

const SCHEMA_MAP: &[(&str, &str)] = &[
    ("manifest", "manifest.schema.json"),
    ("nodes", "nodes.schema.json"),
    ("node", "nodes.schema.json"),
    ("flows", "flows.schema.json"),
    ("flow", "flows.schema.json"),
    ("views", "views.schema.json"),
    ("view", "views.schema.json"),
    ("data", "data-classification.schema.json"),
    ("risks", "risks.schema.json"),
    ("risk", "risks.schema.json"),
    ("decisions", "decisions.schema.json"),
    ("decision", "decisions.schema.json"),
    ("glossary", "glossary.schema.json"),
    ("releases", "release-index.schema.json"),
    ("release", "release-detail.schema.json"),
];

fn schema_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("ARCHITEXT_SCHEMA_DIR") {
        return std::path::PathBuf::from(d);
    }
    std::path::PathBuf::from("viewer").join("schema")
}

pub fn run(topic: &str) {
    let normalized = if topic.is_empty() { "overview" } else { &topic.to_lowercase() };

    let schema_file = SCHEMA_MAP
        .iter()
        .find(|(key, _)| *key == normalized)
        .map(|(_, file)| *file);

    let Some(schema_file) = schema_file else {
        // JS: console.log("Architext data is split across …") — overview or unknown topic
        println!(
            "Architext data is split across manifest, nodes, flows, views, data classification, decisions, risks, glossary, and optional releases JSON files."
        );
        return;
    };

    let path = schema_dir().join(schema_file);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("Cannot read schema {}: {err}", path.display());
            process::exit(1);
        }
    };
    let schema: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("Cannot parse schema {}: {err}", path.display());
            process::exit(1);
        }
    };

    // JS: console.log(`${normalized}: package schema ${schemaFile}`)
    println!("{normalized}: package schema {schema_file}");

    // JS: if (schema.required?.length) console.log(`Required fields: ${schema.required.join(", ")}`)
    if let Some(required) = schema["required"].as_array() {
        if !required.is_empty() {
            let fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
            println!("Required fields: {}", fields.join(", "));
        }
    }
}
