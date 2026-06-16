//! `domain_dump <op> <fixture.json>` — parity gate for domain module ports.
//!
//! Reads the fixture JSON (contains the function input args), runs the
//! corresponding Rust domain function, and prints to stdout EITHER the result
//! JSON or `{"__error__":"<message>"}` on a domain error.
//!
//! Dispatch table: slices B-D extend by adding arms here.

use std::{env, fs, process};

use serde_json::Value;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: domain_dump <op> <fixture.json>");
        process::exit(1);
    }
    let op = &args[1];
    let path = &args[2];

    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Failed to read {path}: {e}");
        process::exit(1);
    });
    let fixture: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("Failed to parse {path}: {e}");
        process::exit(1);
    });

    let result = dispatch(op, &fixture);
    println!("{}", serde_json::to_string(&result).unwrap());
}

fn ok(v: Value) -> Value {
    v
}

fn err(msg: String) -> Value {
    serde_json::json!({ "__error__": msg })
}

fn dispatch(op: &str, fixture: &Value) -> Value {
    use architext_core::domain::{c4_quality, notes, release, rules, schema_migration};

    match op {
        // ── rules ──────────────────────────────────────────────────────────
        "rules.ordered" => {
            let rules_arr = fixture["rules"].as_array()
                .map(|a| a.as_slice())
                .unwrap_or(&[]);
            ok(Value::Array(rules::ordered_rules(rules_arr)))
        }

        "rules.upsert" => {
            let doc = &fixture["document"];
            let rule = &fixture["rule"];
            match rules::upsert_rule(doc, rule) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        "rules.delete" => {
            let doc = &fixture["document"];
            let id = fixture["id"].as_str().unwrap_or("");
            match rules::delete_rule(doc, id) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        "rules.move" => {
            let doc = &fixture["document"];
            let id = fixture["id"].as_str().unwrap_or("");
            let direction = fixture["direction"].as_str().unwrap_or("");
            match rules::move_rule(doc, id, direction) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        "rules.moveBefore" => {
            let doc = &fixture["document"];
            let id = fixture["id"].as_str().unwrap_or("");
            let before_id = fixture["beforeId"].as_str().unwrap_or("");
            match rules::move_rule_before(doc, id, before_id) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        // ── notes ──────────────────────────────────────────────────────────
        "notes.upsert" => {
            let doc = &fixture["document"];
            let note = &fixture["note"];
            match notes::upsert_note(doc, note) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        "notes.delete" => {
            let doc = &fixture["document"];
            let id = fixture["id"].as_str().unwrap_or("");
            match notes::delete_note(doc, id) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        "notes.forTarget" => {
            let notes_arr = fixture["notes"].as_array()
                .map(|a| a.as_slice())
                .unwrap_or(&[]);
            let kind = fixture["kind"].as_str().unwrap_or("");
            let id = fixture["id"].as_str().unwrap_or("");
            ok(Value::Array(notes::notes_for_target(notes_arr, kind, id)))
        }

        // ── schema migrations ───────────────────────────────────────────────
        "schema.migrationPlan" => {
            let current = fixture["currentVersion"].as_str().unwrap_or("");
            let target = fixture["targetVersion"].as_str().unwrap_or("");
            ok(schema_migration::schema_migration_plan(current, target))
        }

        // ── c4 quality ─────────────────────────────────────────────────────────
        "c4.issuesForView" => {
            let view = &fixture["view"];
            let nodes = fixture["nodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            let node_map = c4_quality::build_node_map(nodes);
            let issues = c4_quality::c4_issues_for_view(view, &node_map);
            ok(Value::Array(issues.into_iter().map(Value::String).collect()))
        }

        "c4.drilldownIssues" => {
            let views = fixture["views"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            let nodes = fixture["nodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            let node_map = c4_quality::build_node_map(nodes);
            let issues = c4_quality::c4_drilldown_issues(views, &node_map);
            ok(Value::Array(issues.into_iter().map(Value::String).collect()))
        }

        "c4.repairViews" => {
            let views = fixture["views"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            let nodes = fixture["nodes"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            let node_map = c4_quality::build_node_map(nodes);
            ok(c4_quality::repair_c4_views(views, &node_map))
        }

        // ── release ─────────────────────────────────────────────────────────
        "release.nextMinorVersion" => {
            let release_index = &fixture["releaseIndex"];
            ok(release::next_minor_version(release_index))
        }

        "release.items" => {
            let detail = &fixture["detail"];
            ok(Value::Array(release::release_items(detail).into_iter().cloned().collect()))
        }

        "release.deriveCounts" => {
            let detail = &fixture["detail"];
            ok(release::derive_release_counts(detail))
        }

        "release.summaryFromDetail" => {
            let detail = &fixture["detail"];
            let file = fixture["file"].as_str().unwrap_or("");
            ok(release::release_summary_from_detail(detail, file))
        }

        "release.generatedIndex" => {
            let existing_index = &fixture["existingIndex"];
            let detail_entries = &fixture["detailEntries"];
            ok(release::generated_release_index(existing_index, detail_entries))
        }

        "release.indexGenerationChanges" => {
            let existing_index = &fixture["existingIndex"];
            let generated_index = &fixture["generatedIndex"];
            ok(release::release_index_generation_changes(existing_index, generated_index))
        }

        "release.build" => {
            match release::build_release_plan(fixture) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }

        "release.merge" => {
            let existing = &fixture["existingDetail"];
            let proposed = &fixture["proposedDetail"];
            ok(release::merge_existing_release_plan(existing, proposed))
        }

        "release.changes" => {
            ok(release::release_plan_changes(fixture))
        }

        "release.saveDraft" => {
            ok(release::save_release_plan_draft(fixture))
        }

        "release.approve" => {
            ok(release::approve_release_plan(fixture))
        }

        _ => {
            eprintln!("Unknown op: {op}");
            process::exit(1);
        }
    }
}
