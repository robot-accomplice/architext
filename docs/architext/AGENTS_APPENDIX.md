# Architext Agent Instructions Appendix

Add this section to a target project's `AGENTS.md` or `CLAUDE.md` when adopting
Architext.

```markdown
## Architext Architecture Documentation

This project uses `docs/architext/data/**/*.json` as the machine-readable
architecture and release source of truth.

When changing architecture, data flow, persistence, external integrations, trust
boundaries, deployment topology, observability paths, or major module
responsibilities, update the relevant Architext JSON files before completing the
task.

When release scope, blockers, milestones, posture, evidence, or target dates
change, update Release Truth data under `docs/architext/data/releases/`.
Release Truth is the reviewed release source of truth: completed work,
deferrals, reprioritization, blockers, dependencies, and next actions belong in
the release detail file, with `releases/index.json` refreshed from those facts.
Keep Release Path labels concise and put long context in the selected release
item's detail data. Release Planning is a later Architext 1.3.0 capability; do
not represent unreviewed planning proposals as current Release Truth facts.

For C4 views, keep Context, Container, and Component diagrams at their proper
abstraction level. Prefer splitting dense views over forcing tangled routing,
keep relationship labels visible, and treat duplicate node membership in one
C4 view as a documentation defect to repair in `docs/architext/data/views.json`.

Run the Architext validator after edits:

```sh
architext validate [path]
```

Use the local viewer for review:

```sh
architext serve [path]
```

The optional path defaults to the current directory. Target repositories should
not vendor or edit Architext viewer, schema, tool, package, or Vite files.
Those are owned by the globally installed `architext` package. Edit project
architecture and release data under `docs/architext/data/**/*.json`; use `architext sync
[path]` to install or migrate lifecycle metadata and instructions.

Use `architext doctor [path]` to inspect installation health, including C4
document quality issues, and `architext doctor [path] --yes` to apply
deterministic repairs. `architext sync [path]` runs the same doctor diagnostics
before converging lifecycle state. Use `architext prompt [path]` to print the
current LLM build-out or maintenance instructions.
Do not claim the architecture documentation is current if validation fails or
was skipped.
```
