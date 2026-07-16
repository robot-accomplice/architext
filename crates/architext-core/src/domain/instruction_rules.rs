//! Pure port of `src/domain/lifecycle/instruction-rule-migration.mjs`.
//!
//! All functions operate on plain strings and `serde_json::Value`.
//! I/O-free — callers supply file text.
//!
//! # Lookahead note
//! `managedSectionPattern` in JS is:
//!   `new RegExp(\`\\n?${escaped}[\\s\\S]*?(?=\\n## |$)\`)`
//! The Rust `regex` crate does not support lookahead. The section strip is
//! hand-rolled: locate the heading in the text, then scan forward for the
//! next `\n## ` boundary (or end-of-string). This faithfully replicates the
//! JS semantics without `fancy-regex`.

use indexmap::IndexSet;
use regex::Regex;
use serde_json::{json, Value};

// ─── Constants ────────────────────────────────────────────────────────────────

pub const INSTRUCTION_RULE_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", ".cursorrules"];

const ARCHITEXT_RULE_POINTER_HEADING: &str = "## Architext Project Rules";
const ARCHITEXT_INSTRUCTION_HEADING: &str = "## Architext Architecture Documentation";

const POINTER_BODY: &str = "\
## Architext Project Rules\n\
\n\
Project rules are maintained in `docs/architext/data/rules.json`.\n\
Read that file before changing code, update it when project rules change, and run `architext validate [path]` after Architext data edits.\n\
Do not duplicate long-lived project rules across model-specific instruction files.";

// ─── Section strip — hand-rolled lookahead replacement ───────────────────────

/// Replaces ALL occurrences of a managed section headed by `heading` with
/// `replacement`. Mirrors the global `replace(managedSectionPattern(heading, "g"), replacement)`.
///
/// The JS pattern `\n?<heading>[\s\S]*?(?=\n## |$)` means:
///   - optional leading `\n`
///   - the heading literal
///   - non-greedy `[\s\S]*?` until a lookahead `\n## ` or end-of-string
///
/// Hand-rolled: scan for each occurrence of `heading`, include any preceding
/// `\n`, then extend the match to the next `\n## ` boundary or end-of-string.
fn without_managed_section_global(text: &str, heading: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    loop {
        // Find heading in remaining text
        let Some(rel_pos) = remaining.find(heading) else {
            result.push_str(remaining);
            break;
        };

        // Determine start of match: include optional preceding '\n'
        let match_start = if rel_pos > 0 && remaining.as_bytes()[rel_pos - 1] == b'\n' {
            rel_pos - 1
        } else {
            rel_pos
        };

        // Everything before the match
        result.push_str(&remaining[..match_start]);

        // Find end of match: the section continues until the next `\n## ` or end
        let section_start = rel_pos + heading.len();
        let match_end = find_next_section_boundary(remaining, section_start);

        // Append replacement (no trailing content — the boundary `\n## ` is NOT consumed)
        result.push_str(replacement);

        remaining = &remaining[match_end..];
    }
    result
}

/// Replaces the FIRST occurrence of the managed section with `replacement`.
/// Mirrors the non-global `replace(managedSectionPattern(heading), replacement)`.
fn without_managed_section_first(text: &str, heading: &str, replacement: &str) -> String {
    let Some(rel_pos) = text.find(heading) else {
        return text.to_string();
    };

    let match_start = if rel_pos > 0 && text.as_bytes()[rel_pos - 1] == b'\n' {
        rel_pos - 1
    } else {
        rel_pos
    };

    let section_start = rel_pos + heading.len();
    let match_end = find_next_section_boundary(text, section_start);

    let mut result = String::with_capacity(text.len());
    result.push_str(&text[..match_start]);
    result.push_str(replacement);
    result.push_str(&text[match_end..]);
    result
}

/// Scan forward from `start` in `text` to find the next `\n## ` boundary.
/// Returns the position of the `\n` in `\n## ` (i.e. the boundary is not consumed).
/// If none found, returns `text.len()`.
fn find_next_section_boundary(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = start;
    while i < len {
        // Look for `\n## ` (newline + "## ")
        if bytes[i] == b'\n' && i + 3 < len && &bytes[i + 1..i + 4] == b"## " {
            return i;
        }
        i += 1;
    }
    len
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// `stripMarkdown(value)` — remove bold, inline code, links; collapse whitespace.
fn strip_markdown(value: &str) -> String {
    // **bold** → $1
    let re_bold = Regex::new(r"\*\*(.*?)\*\*").unwrap();
    let s = re_bold.replace_all(value, "$1");
    // `code` → $1
    let re_code = Regex::new(r"`([^`]+)`").unwrap();
    let s = re_code.replace_all(&s, "$1");
    // [text](url) → $1
    let re_link = Regex::new(r"\[(.*?)\]\([^)]*\)").unwrap();
    let s = re_link.replace_all(&s, "$1");
    // collapse whitespace
    let re_ws = Regex::new(r"\s+").unwrap();
    let s = re_ws.replace_all(&s, " ");
    s.trim().to_string()
}

/// `withoutArchitextManagedSections(text)`.
fn without_architext_managed_sections(text: &str) -> String {
    let s = without_managed_section_global(text, ARCHITEXT_RULE_POINTER_HEADING, "\n");
    let s = without_managed_section_global(&s, ARCHITEXT_INSTRUCTION_HEADING, "\n");
    s.trim().to_string()
}

/// `ruleLinesFromText(text)` — extract bullet/numbered list items that pass filters.
fn rule_lines_from_text(text: &str) -> Vec<String> {
    let cleaned = without_architext_managed_sections(text);
    let re_list = Regex::new(r"(?m)^\s*(?:[-*]|\d+[.)]) +(.+?)\s*$").unwrap();
    let re_architext = Regex::new(r"(?i)^architext\b").unwrap();
    let re_proj_rules = Regex::new(r"(?i)^project rules are maintained\b").unwrap();

    let mut seen: IndexSet<String> = IndexSet::new();
    for line in cleaned.split('\n') {
        // Also handle \r\n — strip trailing \r
        let line = line.trim_end_matches('\r');
        if let Some(cap) = re_list.captures(line) {
            let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let stripped = strip_markdown(raw);
            if stripped.len() < 16 { continue; }
            if re_architext.is_match(&stripped) { continue; }
            if re_proj_rules.is_match(&stripped) { continue; }
            // Dedup preserving first occurrence (IndexSet)
            seen.insert(stripped);
        }
    }
    seen.into_iter().collect()
}

/// `isSimpleRuleList(text)` — every non-blank line after stripping managed sections
/// must be a list item or heading.
fn is_simple_rule_list(text: &str) -> bool {
    let body = without_architext_managed_sections(text);
    if body.is_empty() { return true; }
    let re_list = Regex::new(r"^([-*]|\d+[.)]) ").unwrap();
    let re_heading = Regex::new(r"^#{1,3} ").unwrap();
    let meaningful: Vec<&str> = body
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if meaningful.is_empty() { return true; }
    meaningful.iter().all(|line| re_list.is_match(line) || re_heading.is_match(line))
}

/// `normalizeRuleText(value)` — strip markdown, lowercase, collapse whitespace.
fn normalize_rule_text(value: &str) -> String {
    let stripped = strip_markdown(value);
    let lower = stripped.to_lowercase();
    let re_ws = Regex::new(r"\s+").unwrap();
    re_ws.replace_all(lower.trim(), " ").to_string()
}

/// `titleForRule(summary)` — first sentence up to 54 chars; truncate with `...`.
fn title_for_rule(summary: &str) -> String {
    // JS: summary.split(/[.;:]/)[0]?.trim() || summary
    let first_sentence = summary
        .split(['.', ';', ':'])
        .next()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(summary);
    let first_sentence = if first_sentence.is_empty() { summary } else { first_sentence };
    // length is UTF-16 code units — all ASCII in practice for current fixtures
    if first_sentence.chars().count() <= 54 {
        first_sentence.to_string()
    } else {
        // slice(0,51) then trim trailing whitespace + "..."
        let truncated: String = first_sentence.chars().take(51).collect();
        format!("{}...", truncated.trim_end())
    }
}

/// `slugForRule(summary, existingIds)` — slug + collision suffix.
/// `existingIds` is mutated: the chosen id is inserted before returning.
fn slug_for_rule(summary: &str, existing_ids: &mut IndexSet<String>) -> String {
    let stripped = strip_markdown(summary).to_lowercase();
    let re_non_alnum = Regex::new(r"[^a-z0-9]+").unwrap();
    let base_raw = re_non_alnum.replace_all(&stripped, "-");
    // trim leading/trailing dashes
    let base_trimmed = base_raw.trim_matches('-');
    // slice(0,48) — UTF-16 code units; ASCII-only
    let base_48: String = base_trimmed.chars().take(48).collect();
    // trim trailing dashes again (slice may end mid-run)
    let base = base_48.trim_end_matches('-');
    let base = if base.is_empty() { "project-rule" } else { base };

    let mut id = base.to_string();
    let mut suffix = 2u32;
    while existing_ids.contains(&id) {
        id = format!("{}-{}", base, suffix);
        suffix += 1;
    }
    existing_ids.insert(id.clone());
    id
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// `plannedInstructionRuleMigration({ files, existingRules })`.
pub fn planned_instruction_rule_migration(files: &[Value], existing_rules: &[Value]) -> Value {
    let mut existing_ids: IndexSet<String> = existing_rules
        .iter()
        .filter_map(|r| r["id"].as_str().map(|s| s.to_string()))
        .collect();
    let mut existing_summaries: IndexSet<String> = existing_rules
        .iter()
        .filter_map(|r| r["summary"].as_str().map(normalize_rule_text))
        .collect();

    // JS: nextOrderBase = existingRules.reduce((max,r)=>Math.max(max, Number(r.order)||0), 0)
    let next_order_base: i64 = existing_rules.iter().fold(0i64, |max, r| {
        let n = match &r["order"] {
            Value::Number(n) => n.as_f64().unwrap_or(0.0) as i64,
            Value::String(s) => s.parse::<f64>().unwrap_or(0.0) as i64,
            _ => 0,
        };
        if n > max { n } else { max }
    });

    let mut candidate_rules: Vec<Value> = Vec::new();
    let mut rewrite_files: Vec<Value> = Vec::new();
    let mut ambiguous_files: Vec<Value> = Vec::new();

    for file in files {
        let path = file["path"].as_str().unwrap_or("");
        let text = file["text"].as_str().unwrap_or("");

        let rule_lines = rule_lines_from_text(text);
        if rule_lines.is_empty() { continue; }

        let simple = is_simple_rule_list(text);
        let replacement = if simple {
            POINTER_BODY.to_string()
        } else {
            upsert_rule_pointer(text)
        };
        // Convergence gate (code-rca B-2): only plan a rewrite when it would
        // actually change the file's written bytes, or when this file
        // contributes a not-yet-migrated rule below. The unconditional push
        // made doctor re-advertise and phantom-apply the identical rewrite on
        // every run for any already-migrated file (bullets never leave the
        // file by design, so this branch re-entered forever). Compare with the
        // trailing newline the writer ensures, so a missing final newline
        // still converges after one real write.
        let ensure_nl = |s: &str| if s.ends_with('\n') { s.to_string() } else { format!("{s}\n") };
        let rewrite_differs = ensure_nl(&replacement) != ensure_nl(text);
        let candidates_before = candidate_rules.len();

        for summary in rule_lines {
            let normalized = normalize_rule_text(&summary);
            if existing_summaries.contains(&normalized) { continue; }
            existing_summaries.insert(normalized);

            let order = next_order_base + (candidate_rules.len() as i64) * 10 + 10;
            let id = slug_for_rule(&summary, &mut existing_ids);
            let title = title_for_rule(&summary);
            candidate_rules.push(json!({
                "id": id,
                "title": title,
                "summary": summary,
                "category": "Agent Collaboration",
                "criticality": "medium",
                "order": order,
                "source": "maintainer",
                "rationale": format!("Migrated from {} so model-specific instruction files can point at one model-agnostic rules source.", path),
                "appliesTo": ["agent instructions", "project rules"],
                "protection": { "edit": false, "delete": false }
            }));
        }

        let contributed_new = candidate_rules.len() > candidates_before;
        if rewrite_differs || contributed_new {
            rewrite_files.push(json!({ "path": path, "replacement": replacement }));
            if !simple {
                ambiguous_files.push(json!({
                    "path": path,
                    "reason": "preserving non-list prose outside the Architext rule pointer"
                }));
            }
        }
    }

    let repair_changes: Vec<Value> = candidate_rules
        .iter()
        .map(|r| Value::String(format!("migrate instruction rule: {}", r["title"].as_str().unwrap_or(""))))
        .chain(
            rewrite_files.iter().map(|f| {
                Value::String(format!("rewrite {} to point at docs/architext/data/rules.json", f["path"].as_str().unwrap_or("")))
            })
        )
        .collect();

    json!({
        "candidateRules": candidate_rules,
        "rewriteFiles": rewrite_files,
        "ambiguousFiles": ambiguous_files,
        "repairChanges": repair_changes,
    })
}

/// `upsertRulePointer(text)`.
pub fn upsert_rule_pointer(text: &str) -> String {
    let existing = text.trim_end();
    if let Some(heading_pos) = existing.find(ARCHITEXT_RULE_POINTER_HEADING) {
        // Replace the first managed section with pointerBody, trimEnd, + "\n".
        // The splice consumes one newline before the heading, so re-supply the
        // separator when the heading is not at the start of the file —
        // otherwise re-application eats the blank line the append branch
        // inserted and a freshly migrated file triggers one more rewrite
        // (non-idempotence found by the convergence fixed-point test, B-2).
        let replacement = if heading_pos > 0 {
            format!("\n{POINTER_BODY}")
        } else {
            POINTER_BODY.to_string()
        };
        let replaced =
            without_managed_section_first(existing, ARCHITEXT_RULE_POINTER_HEADING, &replacement);
        format!("{}\n", replaced.trim_end())
    } else {
        if existing.is_empty() {
            format!("{}\n", POINTER_BODY)
        } else {
            format!("{}\n\n{}\n", existing, POINTER_BODY)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_markdown_bold() {
        assert_eq!(strip_markdown("**hello** world"), "hello world");
    }

    #[test]
    fn strip_markdown_code() {
        assert_eq!(strip_markdown("`foo` bar"), "foo bar");
    }

    #[test]
    fn strip_markdown_link() {
        assert_eq!(strip_markdown("[text](http://example.com)"), "text");
    }

    #[test]
    fn title_short() {
        assert_eq!(title_for_rule("Short rule"), "Short rule");
    }

    #[test]
    fn title_truncate() {
        let long = "A very long rule summary that exceeds fifty-four characters total";
        let t = title_for_rule(long);
        assert!(t.ends_with("..."));
        // before "..." should be <= 51 chars of content
        let content = t.strip_suffix("...").unwrap();
        assert!(content.chars().count() <= 51);
    }

    #[test]
    fn title_split_on_period() {
        assert_eq!(title_for_rule("First sentence. Second sentence"), "First sentence");
    }

    #[test]
    fn slug_basic() {
        let mut ids: IndexSet<String> = IndexSet::new();
        let s = slug_for_rule("Hello World", &mut ids);
        assert_eq!(s, "hello-world");
        assert!(ids.contains("hello-world"));
    }

    #[test]
    fn slug_collision() {
        let mut ids: IndexSet<String> = IndexSet::new();
        ids.insert("hello-world".to_string());
        let s = slug_for_rule("Hello World", &mut ids);
        assert_eq!(s, "hello-world-2");
    }

    #[test]
    fn slug_max_48() {
        let mut ids: IndexSet<String> = IndexSet::new();
        let summary = "A very long rule summary that exceeds forty eight characters definitely";
        let s = slug_for_rule(summary, &mut ids);
        assert!(s.len() <= 48);
    }

    #[test]
    fn upsert_empty_text() {
        let result = upsert_rule_pointer("");
        assert!(result.starts_with(ARCHITEXT_RULE_POINTER_HEADING));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn upsert_append() {
        let text = "# File\n\n- Some rule\n";
        let result = upsert_rule_pointer(text);
        assert!(result.contains(ARCHITEXT_RULE_POINTER_HEADING));
        assert!(result.starts_with("# File"));
    }

    #[test]
    fn upsert_replace_heading() {
        let text = "# File\n\n## Architext Project Rules\n\nOld content.\n\n## Other\n\nOther.\n";
        let result = upsert_rule_pointer(text);
        assert!(!result.contains("Old content."));
        assert!(result.contains(ARCHITEXT_RULE_POINTER_HEADING));
        assert!(result.contains("## Other"));
    }

    #[test]
    fn is_simple_rule_list_all_bullets() {
        assert!(is_simple_rule_list("- Rule one\n- Rule two\n"));
    }

    #[test]
    fn is_simple_rule_list_with_prose() {
        assert!(!is_simple_rule_list("Some prose.\n- Rule one\n"));
    }

    #[test]
    fn planned_migration_simple_list() {
        let files = vec![json!({
            "path": "CLAUDE.md",
            "text": "- Always check tests before committing any code changes\n- Keep functions small and focused on single responsibility\n"
        })];
        let result = planned_instruction_rule_migration(&files, &[]);
        let candidates = result["candidateRules"].as_array().unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0]["order"], 10);
        assert_eq!(candidates[1]["order"], 20);
        // simple list → replacement is just pointerBody (no \n at end here)
        let rewrite = &result["rewriteFiles"].as_array().unwrap()[0];
        assert_eq!(rewrite["replacement"].as_str().unwrap(), POINTER_BODY);
    }

    #[test]
    fn planned_migration_dedup() {
        let files = vec![json!({
            "path": "CLAUDE.md",
            "text": "- Always check tests before committing any code changes\n"
        })];
        let existing = vec![json!({ "id": "e1", "summary": "Always check tests before committing any code changes", "order": 10 })];
        let result = planned_instruction_rule_migration(&files, &existing);
        assert_eq!(result["candidateRules"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn planned_migration_converges_on_fully_migrated_file() {
        // code-rca B-2 (field-hit on roboticus): a file whose bullets are ALL
        // already migrated and whose pointer section is already current must
        // produce NO rewrite, NO ambiguous entry, NO repair changes — the old
        // unconditional push made doctor re-advertise and phantom-apply the
        // identical rewrite forever.
        // The converged form is by definition the rewrite's own output: apply
        // the pointer upsert once (as a real doctor run would write it) and
        // feed that back — the fixed point must advertise nothing.
        let original =
            "# My project\n\nSome prose the maintainer wrote.\n\n- Always check tests before committing any code changes\n";
        let text = upsert_rule_pointer(original);
        let files = vec![json!({ "path": "AGENTS.md", "text": text })];
        let existing = vec![json!({
            "id": "e1",
            "summary": "Always check tests before committing any code changes",
            "order": 10
        })];
        let result = planned_instruction_rule_migration(&files, &existing);
        assert_eq!(result["candidateRules"].as_array().unwrap().len(), 0);
        assert_eq!(
            result["rewriteFiles"].as_array().unwrap().len(),
            0,
            "byte-identical rewrite must not be advertised: {result}"
        );
        assert_eq!(result["ambiguousFiles"].as_array().unwrap().len(), 0);
        assert_eq!(result["repairChanges"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn planned_migration_still_rewrites_when_pointer_missing() {
        // Guard: all bullets deduped but NO pointer section yet — adding the
        // pointer is a genuine one-time change and must still be advertised
        // (it converges on the next run once the pointer is present).
        let files = vec![json!({
            "path": "AGENTS.md",
            "text": "Prose.\n\n- Always check tests before committing any code changes\n"
        })];
        let existing = vec![json!({
            "id": "e1",
            "summary": "Always check tests before committing any code changes",
            "order": 10
        })];
        let result = planned_instruction_rule_migration(&files, &existing);
        assert_eq!(result["candidateRules"].as_array().unwrap().len(), 0);
        assert_eq!(result["rewriteFiles"].as_array().unwrap().len(), 1);
    }
}
