---
name: architext
description: Use when architecture, flows, C4 views, data movement, deployment topology, risks, decisions, project rules, roadmap, release planning, or Release Truth change in a repository that uses docs/architext/data JSON files or the architext CLI; when validating, repairing, installing, syncing, serving, or reviewing Architext data; or when creating LLM-ready architecture documentation from source code.
---

# Architext

Use this skill to keep Architext's machine-readable architecture model current.
Architext data is project-owned JSON, usually under `docs/architext/data/`.
The local viewer is a projection of that data, not the source of truth.

## Core Workflow

1. Read the existing Architext data before editing it.
2. Read source files and project docs before changing architecture claims.
3. Update architecture documentation before claiming implementation work is complete.
4. Reuse existing IDs for existing concepts.
5. Create nodes before referencing them from flows, views, risks, decisions, releases, or rules.
6. Validate after every Architext data change.
7. Do not claim Architext is current if validation failed or was skipped.

Prefer source-backed claims. When extracting architecture from code, first draft
proposed JSON changes with source paths and confidence notes. Do not replace
validation with extracted claims.

## Files To Inspect

In a target repository, expect:

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
    rules.json
    roadmap.json
    releases/
      index.json
      <release-id>.json
```

If these files are missing, use `architext sync [path]` to install or repair the
data-only Architext layout.

The diagram, C4, data, and release sections below are operational summaries. The
project docs are the canonical policy source; when a summary here conflicts with
them, the canonical docs win. Prefer them for full detail:

- `docs/architecture/LLM_ARCHITEXT.md`
- `docs/architecture/AGENTS_APPENDIX.md`
- `docs/architecture/ARCHITECTURE_PLAN.md`
- `docs/architext/schema/*.json`

## Commands

Use the scoped package CLI:

```sh
architext validate [path]
architext doctor [path]
architext doctor [path] --yes
architext sync [path]
architext sync [path] --dry-run
architext serve [path]
architext prompt [path]
```

The optional path defaults to the current directory.

Do not edit copied viewer, schema, package, Vite, or tool files in target
repositories. In Architext 1.0+, those are package-owned. Edit target-owned
architecture, rule, roadmap, and release data under `docs/architext/data/`.

## Update Triggers

Update Architext when changing:

- module, service, job, worker, queue, or data-store responsibilities
- public or internal APIs
- ordered business, runtime, or infrastructure flows
- data movement, data classification, or sensitive data handling
- authentication, authorization, trust boundaries, or security controls
- deployment topology, runtime dependencies, or observability paths
- external integrations
- architecture decisions or known architecture risks
- project rules, roadmap items, release scope, blockers, milestones, posture, or evidence

## Flow Diagrams

Keep flows ordered and traceable. Every rendered node, edge, marker, and label
must be traceable to the selected flow, a selected supporting relationship, or
an explicit context relationship in the projection. Remove disconnected context,
connect it with a labeled relationship, or split it into another view. Do not
leave loose boxes, endpoints, markers, labels, or other orphaned elements for
the reader to interpret.

Prefer semantic iconography over UML/code diagrams or broad flowchart shape
palettes. Use `step.kind` for flow semantics such as `start`, `process`,
`decision`, `async`, `persistence`, `artifact`, `return`, and `stop`.

For decision branches:

- Separate the decision from component nodes.
- Add at least two outgoing outcome steps from the decision point.
- Set `step.outcome` to concrete branch labels such as `valid`, `invalid`,
  `approved`, `rejected`, `cache hit`, or `cache miss`.
- Make branch lines share the decision step number.
- Ensure both the decision source and each branch destination are visible and
  highlighted when the decision step is selected.
- Do not model one step as both a component interaction and a decision point.

Use `workflow` views in `views.json` when ordered work or use-case paths need a
dedicated Flows projection. Workflow views should reference existing nodes and
selected flows instead of duplicating flow facts.

## Sequence Diagrams

Sequence diagrams must make round trips explicit when the flow requires them.
Create return paths for request/response, command/result, event/acknowledgement,
and failure-return interactions.

For return paths:

- Use `kind: "return"` for return steps.
- Set `returnOf` when the return answers a specific outbound step.
- Keep outbound and inbound lines solid unless the renderer's contract says
  otherwise.
- Use activation bars or transaction framing when a participant owns an active
  operation across outbound and return messages.

Use `sequenceFrames` for loops, retries, optional branches, and transaction or
consistency blocks. Frames should connect outbound and return paths so the
reader does not have to infer grouping from nearby lines.

## C4 Views

Keep C4 Context, Container, Component, and Code views at their proper
abstraction levels. Split dense C4 views instead of hiding labels or accepting
tangled routing.

Use explicit `scopeNodeId` metadata for drilldown chains:

- Context system node -> scoped Container view
- decomposable Container node -> scoped Component view
- decomposable Component node -> scoped Code view when code-level documentation exists

Do not invent child diagrams for actors, external dependencies, or nodes outside
the project boundary. Repair duplicate node membership in a single C4 view by
updating `views.json`.

## Data, Risks, Decisions, Rules, And Releases

Update data classification whenever data movement changes.

Update risks when adding external dependencies, persistence, async processing,
sensitive data handling, trust boundary crossings, or operational complexity.

Update decisions when the work creates or changes meaningful architectural
tradeoffs.

Update `rules.json` when project rules change. Rule categories are maintainer
defined. Respect `protection.edit` and `protection.delete`; protected rules are
not casual cleanup targets. Rank rules by `criticality` and `order`, not
alphabetical order or creation time.

Update Release Truth under `docs/architext/data/releases/` when release scope,
blockers, milestones, posture, evidence, target dates, completion, deferral, or
reprioritization changes. Do not leave completed, deferred, or cut release items
in active blocker `itemIds`. Completion and blocking are mutually exclusive.

Use `roadmap.json` for release planning source items. Manually entered release
scope uses `source: "ad-hoc"` and should be promoted into `roadmap.json` when
the release plan is approved.

## Validation Standard

Run:

```sh
architext validate [path]
```

If working inside the Architext package repository, also use the repository's
test and build commands that match the touched surface, such as:

```sh
npm run validate
npm run build
npm test
```

Report skipped validation explicitly. Broken Architext data is worse than
missing Architext data because it gives future agents and humans false
confidence.
