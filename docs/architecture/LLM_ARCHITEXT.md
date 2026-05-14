# LLM Architext Contract

This file is written for LLM agents working inside a project that uses
Architext.

## Purpose

Architext JSON files are the machine-readable architecture source of truth for
the project. They describe components, dataflows, deployment/runtime structure,
data classification, risks, and architecture decisions.

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
- Prefer source-path-backed claims.
- Mark uncertainty explicitly instead of inventing details.
- Run `architext validate [path]` after changing data.
- Do not claim Architext is current if validation failed or was skipped.
- Do not edit copied viewer, schema, package, Vite, or local tool files in a
  target repository. Those files are package-owned in Architext 1.0+.

## Persistence Rules

Persist these project-owned files in git:

- `docs/architext/data/*.json`
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
  schema/
    *.schema.json
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
