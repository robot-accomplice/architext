# LLM Architext Contract

This file is written for LLM agents working inside a project that uses
Architext.

## Purpose

Architext JSON files are the machine-readable architecture source of truth for
the project. They describe components, dataflows, deployment/runtime structure,
data classification, risks, and architecture decisions.

`docs/architext/data/manifest.json` records the Architext data schema version.
That version tracks the JSON data contract, not the installed CLI/package
version. Additive schema changes may ship in minor releases; breaking schema
changes require a major semver release and an Architext-managed migration path.

When architecture changes, update Architext data before claiming the task is
complete.

## Required Behavior

- Read existing Architext data before editing it.
- Reuse existing IDs for existing concepts.
- Create new nodes before referencing them from flows or views.
- Keep flows ordered.
- Update data classification whenever data movement changes.
- Update risks when adding external dependencies, persistence, async
  processing, sensitive data handling, trust boundary crossings, or operational
  complexity.
- Update Release Truth data under `docs/architext/data/releases/` when release
  scope, blockers, milestones, posture, evidence, or target dates change.
- Treat Release Truth as the reviewed release source of truth. If you complete,
  defer, add, remove, reprioritize, or block release work, update the release
  detail file and ensure `releases/index.json` is regenerated from those facts.
- Keep Release Path rows concise. Put rationale, blocker explanation,
  dependency detail, evidence, and next actions in the selected release item's
  detail data instead of duplicating long prose in labels or summaries.
- Use `docs/architext/data/roadmap.json` for release planning source items.
  Selected roadmap items use `source: "roadmap"` in release detail data.
  Manually entered scope uses `source: "ad-hoc"` and must be promoted into
  `roadmap.json` when the release plan is approved.
- Do not use unreviewed Release Planning proposals as current release facts.
  Release Planning writes approved proposals into the same Release Truth JSON
  model.
- Prefer source-path-backed claims.
- Mark uncertainty explicitly instead of inventing details.
- Keep C4 Context, Container, and Component views at their proper abstraction
  level; split dense C4 views instead of hiding labels or relying on tangled
  routing.
- Repair duplicate node membership in a single C4 view by updating
  `docs/architext/data/views.json`.
- Prefer `architext doctor [path]` or `architext sync [path] --dry-run` before
  manual C4 view repair. Use `architext doctor [path] --yes` or `architext sync
  [path] --yes` when deterministic repairs are sufficient.
- Run `architext validate [path]` after changing data.
- Do not claim Architext is current if validation failed or was skipped.
- Do not edit copied viewer, schema, package, Vite, or local tool files in a
  target repository. Those files are package-owned in Architext 1.0+.

## Persistence Rules

Persist these project-owned files in git:

- `docs/architext/data/*.json`
- `docs/architext/data/roadmap.json`
- `docs/architext/data/releases/*.json`
- `docs/architext/.architext.json`
- repository-level `AGENTS.md` or `CLAUDE.md` Architext instructions, when
  present

Do not persist generated or local runtime artifacts:

- `docs/architext/dist/`
- `.DS_Store`
- editor/OS temp files
- local server logs
- screenshots created only for debugging unless intentionally added to project
  documentation

If the target project does not already ignore generated artifacts, use
`architext sync [path]` to update lifecycle metadata, instructions, and ignore
rules.

## Files

Expected project-local location:

```text
docs/architext/
  data/
    manifest.json
    nodes.json
    flows.json
    views.json
    data-classification.json
    decisions.json
    risks.json
    glossary.json
    roadmap.json
    releases/
      index.json
      <release-id>.json
```

## Update Triggers

Update Architext when changing:

- module or service responsibilities
- public APIs
- internal APIs
- queues, topics, jobs, or workers
- data stores
- external integrations
- authentication or authorization behavior
- trust boundaries
- deployment topology
- observability paths
- sensitive data handling
- core business flows
- architecture decisions
- known architecture risks

## Validation Rule

Validation is not optional. Broken Architext data is worse than missing
Architext data because it gives humans and future LLMs false confidence.
