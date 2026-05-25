# Architext Agent Instructions Appendix

Add this section to a target project's `AGENTS.md` or `CLAUDE.md` when adopting
Architext.

```markdown
## Architext Architecture Documentation

This project uses `docs/architext/data/**/*.json` as the machine-readable
architecture and release source of truth.

`docs/architext/data/manifest.json` records the Architext data schema version.
That version tracks the JSON data contract, not the installed CLI/package
version. Additive schema changes may ship in minor releases; breaking schema
changes require a major semver release and an Architext-managed migration path.

When changing architecture, data flow, persistence, external integrations, trust
boundaries, deployment topology, observability paths, or major module
responsibilities, update the relevant Architext JSON files before completing the
task.

When release scope, blockers, milestones, posture, evidence, or target dates
change, update Release Truth data under `docs/architext/data/releases/`.
Release Truth is the reviewed release source of truth: completed work,
deferrals, reprioritization, blockers, dependencies, and next actions belong in
the release detail file, with `releases/index.json` refreshed from those facts.
Do not leave completed, deferred, or cut release items in active blocker
`itemIds`; completion and blocking are mutually exclusive lifecycle facts.
Keep Release Path labels concise and put long context in the selected release
item's detail data.

When planning a future release, use `docs/architext/data/roadmap.json` as the
roadmap source and Release Planning as the approval boundary. Selected roadmap
items keep `source: "roadmap"`; manually entered scope uses `source:
"ad-hoc"` and should be promoted into `roadmap.json` when the plan is approved.
Do not represent unreviewed planning proposals as current Release Truth facts.

When project rules change, update `docs/architext/data/rules.json`.
Categories are maintainer-defined classifications such as Architecture,
Development, Design, Release, or any project-specific grouping. Respect
`protection.edit` and `protection.delete`; protected rules are not casual
cleanup targets. Rank rules by `criticality` and `order`, not alphabetical
order or creation time.

When ordered work or use-case paths deserve a dedicated Flows projection, add a
`workflow` view in `docs/architext/data/views.json`. Workflow views should reuse
existing nodes and ordered flows; do not duplicate flow facts or invent
workflow-specific routing rules.

For source extraction work, produce a reviewable draft of proposed JSON changes
with source paths and confidence notes before editing data files. Never replace
validation with extracted claims.

For C4 views, keep Context, Container, and Component diagrams at their proper
abstraction level. Prefer splitting dense views over forcing tangled routing,
keep relationship labels visible, and treat duplicate node membership in one
C4 view as a documentation defect to repair in `docs/architext/data/views.json`.
Use explicit `scopeNodeId` metadata to make C4 drilldown navigable: a Context
node that represents the system should have a scoped Container view, a
decomposable Container node should have a scoped Component view, and a
decomposable Component node should have a scoped Code view when code-level
documentation exists. If a node is external or intentionally outside the
project boundary, leave it without a child view so the viewer can explain that
drilldown is unavailable.

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
architecture, roadmap, and release data under `docs/architext/data/**/*.json`;
use `architext sync [path]` to install or migrate lifecycle metadata and
instructions.

Use `architext doctor [path]` to inspect installation health, including C4
document quality issues, and `architext doctor [path] --yes` to apply
deterministic repairs. `architext sync [path]` runs the same doctor diagnostics
before converging lifecycle state. Use `architext prompt [path]` to print the
current agent build-out or maintenance instructions.
Do not claim the architecture documentation is current if validation fails or
was skipped.
```
