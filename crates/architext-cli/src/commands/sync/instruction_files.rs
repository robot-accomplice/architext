//! Port of `upsertInstructionFile` and `appendixMarkdown` / `replaceArchitextSection`
//! from `architext-cli.mjs`.

use std::path::Path;

/// The instruction text inserted into AGENTS.md / CLAUDE.md.
///
/// This is the content of `viewer/AGENTS_APPENDIX.md` extracted from the
/// ```markdown ... ``` fence, trimmed. It must match the JS `appendixMarkdown()`
/// result exactly.
///
/// Source: viewer/AGENTS_APPENDIX.md (the text between the first ```markdown
/// and the last closing ```).
const APPENDIX: &str = "\
## Architext Architecture Documentation\n\
\n\
This project uses `docs/architext/data/**/*.json` as the machine-readable\n\
architecture and release source of truth.\n\
\n\
Derive what you record in those JSON files from the **source code only**.\n\
Existing architecture documentation — prose READMEs, design docs, diagrams,\n\
comments, and even prior Architext claims — may be stale, aspirational, or\n\
wrong; do not treat any of it as authoritative for what the system actually is.\n\
Read the code to determine real responsibilities, flows, data movement,\n\
dependencies, and trust boundaries. Treat existing documents as unverified\n\
hints at most, verify every claim against the code, and when code and a\n\
document disagree, the code wins. (This governs the architecture you record;\n\
this contract and the schema still govern how you record it.)\n\
\n\
`docs/architext/data/manifest.json` records the Architext data schema version.\n\
That version tracks the JSON data contract, not the installed CLI/package\n\
version. Additive schema changes may ship in minor releases; breaking schema\n\
changes require a major semver release and an Architext-managed migration path.\n\
\n\
When changing architecture, data flow, persistence, external integrations, trust\n\
boundaries, deployment topology, observability paths, or major module\n\
responsibilities, update the relevant Architext JSON files before completing the\n\
task.\n\
\n\
When release scope, blockers, milestones, posture, evidence, or target dates\n\
change, update Release Truth data under `docs/architext/data/releases/`.\n\
Release Truth is the reviewed release source of truth: completed work,\n\
deferrals, reprioritization, blockers, dependencies, and next actions belong in\n\
the release detail file, with `releases/index.json` refreshed from those facts.\n\
Keep Release Path labels concise and put long context in the selected release\n\
item's detail data.\n\
\n\
When planning a future release, use `docs/architext/data/roadmap.json` as the\n\
roadmap source and Release Planning as the approval boundary. Selected roadmap\n\
items keep `source: \"roadmap\"`; manually entered scope uses `source:\n\
\"ad-hoc\"` and should be promoted into `roadmap.json` when the plan is approved.\n\
Do not represent unreviewed planning proposals as current Release Truth facts.\n\
\n\
When project rules change, update `docs/architext/data/rules.json`.\n\
Categories are maintainer-defined classifications such as Architecture,\n\
Development, Design, Release, or any project-specific grouping. Respect\n\
`protection.edit` and `protection.delete`; protected rules are not casual\n\
cleanup targets. Rank rules by `criticality` and `order`, not alphabetical\n\
order or creation time.\n\
\n\
Element notes are human annotations on an architecture element (node, flow,\n\
decision, risk, view, or data class), persisted in the optional\n\
`docs/architext/data/notes.json` and registered as `manifest.files.notes`.\n\
Each note records `target: { kind, id }`, a `category`\n\
(`note` | `mitigation` | `caveat` | `todo`), a `body`, and timestamps; the\n\
note's `target.id` must reference an existing element (validation enforces\n\
this). Notes capture maintainer judgement — for example, that a high-risk\n\
area is intentionally mitigated by the documented system — so treat them as\n\
user-owned: preserve and update them, but do not fabricate notes or delete a\n\
human's note as cleanup. They are edited in the viewer (the detail panel's\n\
Notes section) and never replace validation or recorded architecture facts.\n\
\n\
When ordered work or use-case paths deserve a dedicated Flows projection, add a\n\
`workflow` view in `docs/architext/data/views.json`. Workflow views should reuse\n\
existing nodes and ordered flows; do not duplicate flow facts or invent\n\
workflow-specific routing rules.\n\
\n\
Keep flow diagrams free of orphaned elements. Every rendered node, edge, marker,\n\
and label must be traceable to the selected flow, a selected supporting\n\
relationship, or an explicit context relationship shown in the projection.\n\
Remove disconnected context, connect it with a labeled relationship, or split it\n\
into a separate view; do not leave loose boxes, endpoints, markers, or labels\n\
for the reader to interpret. Prefer semantic iconography over UML/code diagrams\n\
or broad flowchart shape palettes for flow enrichment. Mark decision, start,\n\
stop, async, persistence, artifact, return, and process semantics with\n\
`step.kind` when the flow needs them. For decision branches, set `step.outcome`\n\
to the concrete branch/result label that should be readable on the path. A\n\
decision branch should have at least two outgoing outcome steps from the\n\
decision node, and those branch lines should share the decision step number. Do\n\
not add UML/code diagrams for now.\n\
For sequence diagrams, create explicit return paths\n\
for request/response, command/result, event/acknowledgement, and failure-return\n\
interactions when the flow requires them. Mark return steps with `kind:\n\
\"return\"` and `returnOf` when they answer a specific outbound step. Use\n\
`sequenceFrames` for loops, retries, optional branches, and transaction or\n\
consistency blocks so outbound and return messages are visibly grouped instead\n\
of implied.\n\
\n\
For source extraction work, produce a reviewable draft of proposed JSON changes\n\
with source paths and confidence notes before editing data files. Never replace\n\
validation with extracted claims.\n\
\n\
For C4 views, keep Context, Container, and Component diagrams at their proper\n\
abstraction level. Prefer splitting dense views over forcing tangled routing,\n\
keep relationship labels visible, and treat duplicate node membership in one\n\
C4 view as a documentation defect to repair in `docs/architext/data/views.json`.\n\
Use explicit `scopeNodeId` metadata to make C4 drilldown navigable: a Context\n\
node that represents the system should have a scoped Container view, a\n\
decomposable Container node should have a scoped Component view, and a\n\
decomposable Component node should have a scoped Code view when code-level\n\
documentation exists. If a node is external or intentionally outside the\n\
project boundary, leave it without a child view so the viewer can explain that\n\
drilldown is unavailable.\n\
\n\
Run the Architext validator after edits:\n\
\n\
```sh\n\
architext validate [path]\n\
```\n\
\n\
Use the local viewer for review:\n\
\n\
```sh\n\
architext serve [path]\n\
```\n\
\n\
The optional path defaults to the current directory. Target repositories should\n\
not vendor or edit Architext viewer, schema, tool, package, or Vite files.\n\
Those are owned by the globally installed `architext` package. Edit project\n\
architecture, roadmap, and release data under `docs/architext/data/**/*.json`;\n\
use `architext sync [path]` to install or migrate lifecycle metadata and\n\
instructions.\n\
\n\
Use `architext doctor [path]` to inspect installation health, including C4\n\
document quality issues, and `architext doctor [path] --yes` to apply\n\
deterministic repairs. `architext sync [path]` runs the same doctor diagnostics\n\
before converging lifecycle state. Use `architext prompt [path]` to print the\n\
current agent build-out or maintenance instructions.\n\
Do not claim the architecture documentation is current if validation fails or\n\
was skipped.";

/// Port of JS `replaceArchitextSection(existing, appendix)`.
///
/// Inserts or replaces the `## Architext Architecture Documentation` section.
pub fn replace_architext_section(existing: &str, appendix: &str) -> String {
    let heading = "## Architext Architecture Documentation";
    let start = existing.find(heading);
    if start.is_none() {
        // No existing section — append at end.
        let prefix = existing.trim_end();
        return if prefix.is_empty() {
            format!("{appendix}\n")
        } else {
            format!("{prefix}\n\n{appendix}\n")
        };
    }
    let start = start.unwrap();

    // Find the next `\n## ` boundary after the heading (not a sub-heading `## #`).
    let after_heading = start + heading.len();
    let rest = &existing[after_heading..];
    // JS: existing.slice(start + heading.length).search(/\n## (?!#)/)
    let next_heading_rel = find_next_h2(rest);
    let end = match next_heading_rel {
        Some(rel) => after_heading + rel,
        None => existing.len(),
    };

    let before = existing[..start].trim_end();
    let after_raw = &existing[end..];
    // JS: .replace(/^\n+/, "\n") on the after part
    let after = replace_leading_newlines(after_raw, "\n");

    // JS: `${before}${before ? "\n\n" : ""}${appendix}\n${after}`.replace(/\n{3,}/g, "\n\n")
    let joined = if before.is_empty() {
        format!("{appendix}\n{after}")
    } else {
        format!("{before}\n\n{appendix}\n{after}")
    };
    // collapse 3+ consecutive newlines to 2
    collapse_blank_lines(&joined)
}

/// Find the position of `\n## ` (not followed by `#`) within `text`.
/// Returns the index of the `\n` character.
fn find_next_h2(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'\n'
            && i + 3 < len
            && bytes[i + 1] == b'#'
            && bytes[i + 2] == b'#'
            && bytes[i + 3] == b' '
        {
            // check it's not `## #` (i.e. not a sub-heading `### ` disguised)
            // JS regex: /\n## (?!#)/ — look ahead for `#` after `## `
            if i + 4 < len && bytes[i + 4] == b'#' {
                i += 1;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

fn replace_leading_newlines(s: &str, replacement: &str) -> String {
    let trimmed = s.trim_start_matches('\n');
    if trimmed.len() == s.len() {
        s.to_string()
    } else {
        format!("{replacement}{trimmed}")
    }
}

fn collapse_blank_lines(s: &str) -> String {
    // Replace 3+ consecutive newlines with exactly 2.
    let mut out = String::with_capacity(s.len());
    let mut newline_count = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                out.push(ch);
            }
        } else {
            newline_count = 0;
            out.push(ch);
        }
    }
    out
}

/// Port of `upsertInstructionFile({ target, fileName, dryRun })`.
///
/// Returns `(changed, created)`.
pub fn upsert_instruction_file(target: &Path, file_name: &str, dry_run: bool) -> std::io::Result<(bool, bool)> {
    let destination = target.join(file_name);
    let existing = if destination.exists() {
        std::fs::read_to_string(&destination)?
    } else {
        String::new()
    };
    let next = replace_architext_section(&existing, APPENDIX);
    if next == existing {
        return Ok((false, false));
    }
    if !dry_run {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&destination, next.as_bytes())?;
    }
    let created = existing.is_empty();
    Ok((true, created))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_existing_creates_file() {
        let result = replace_architext_section("", APPENDIX);
        assert!(result.starts_with("## Architext Architecture Documentation"));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn existing_without_section_appends() {
        let existing = "# My Project\n\nSome content.\n";
        let result = replace_architext_section(existing, APPENDIX);
        assert!(result.starts_with("# My Project"));
        assert!(result.contains("## Architext Architecture Documentation"));
        // No triple blank lines
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn idempotent_update() {
        let existing = "# My Project\n\nSome content.\n";
        let first = replace_architext_section(existing, APPENDIX);
        let second = replace_architext_section(&first, APPENDIX);
        assert_eq!(first, second);
    }

    #[test]
    fn replaces_existing_section() {
        let existing = "# My Project\n\n## Architext Architecture Documentation\n\nOld content.\n\n## Other\n\nOther.\n";
        let result = replace_architext_section(existing, APPENDIX);
        assert!(!result.contains("Old content."));
        assert!(result.contains("## Architext Architecture Documentation"));
        assert!(result.contains("## Other"));
    }

    #[test]
    fn section_at_start() {
        let existing = "## Architext Architecture Documentation\n\nOld content.\n";
        let result = replace_architext_section(existing, APPENDIX);
        assert!(result.starts_with("## Architext Architecture Documentation"));
        assert!(!result.contains("Old content."));
    }
}
