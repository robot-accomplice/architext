# Architext Roadmap

This roadmap tracks product and architecture direction for Architext. It is
intentionally separate from `README.md`: the README should explain how to
install and use released capabilities, while this file tracks planned and
in-progress work.

## Guiding Priorities

- Keep target repositories data-only: architecture JSON, lifecycle metadata,
  and optional agent instructions.
- Keep the global CLI as the primary user interface.
- Prefer systemic fixes over localized patches.
- Preserve architecture facts during lifecycle automation.
- Make routing correctness a first-class quality gate.
- Keep public docs focused on user workflow, not protected release operations.

## Current Baseline

Implemented:

- Global CLI lifecycle commands with optional target path defaults:
  `sync`, `doctor`, `serve`, `validate`, `build`, `prompt`, `status`,
  `clean`, `explain`, and `version`.
- Data-only target repository model.
- Copied-install migration path that preserves `docs/architext/data/*.json`.
- Managed Architext agent instruction replacement.
- Package-owned viewer, schemas, validation, and build runtime.
- Static viewer export.
- Orthogonal, spline, and straight line styles through one shared planning
  pipeline.
- C4 context/container/component views and C4 document quality checks.
- Routing fitness tests and benchmark checks.
- Initial clean-architecture refactor seams for CLI adapters, lifecycle domain,
  architecture model validation, viewer adapters, presentation policy, diagram
  planning, and routing primitives.

## Near-Term Roadmap

### Routing Correctness

Goal: make dense diagrams readable without relying on ad hoc view-specific
patches.

- Split the current strategy assembly module into separate orthogonal, spline,
  and straight strategy modules if the style-specific branch bodies continue
  to grow. Geometry, ports, port-pair enumeration, corridor discovery,
  candidate construction, labels, route indexing, rendering, scoring/warnings,
  style normalization, caching, and grid-search priority queue infrastructure
  are already split out.
- Keep a single public routing pipeline while separating style-specific
  candidate generation and scoring internally.
- Fix remaining Deployment and Data/Risks overlapping-line cases through route
  indexing, candidate generation, and scoring.
- Add fitness fixtures for Deployment and Data/Risks density.
- Preserve line-style parity: orthogonal, spline, and straight must all fail
  loudly when a route cannot be made readable.

### Viewer Refactor

Goal: make React render already-shaped presentation models instead of owning
domain and layout policy.

- Extract `AppShell`, top navigation, side panels, canvas header, canvas
  viewport, diagram renderers, step summary, and detail panel components.
- Move detail section formatting into presenter modules.
- Keep browser APIs in hooks/adapters.
- Add visual smoke coverage for panel collapse, canvas containment, line style,
  fit behavior, selected-step highlighting, and details.

### CLI Lifecycle Refactor

Goal: make lifecycle behavior explicit use cases behind adapters.

- Move sync, doctor, validate, build, serve, prompt, and clean execution out of
  `tools/architext-adopt.mjs`.
- Define ports for filesystem, package runtime files, validation runner, git
  state, HTTP serving, and interactive prompts.
- Keep `tools/architext-adopt.mjs` as a thin executable bootstrap.
- Ensure `sync` and `doctor` continue to share one repair derivation path.

### CI And Release Gates

Goal: make releasability verifiable using only this repository's fixtures,
package scripts, and generated package contents.

- Run unit, CLI, C4, routing, and benchmark checks in CI.
- Validate bundled Architext self-data with package-owned schemas.
- Build the package-owned viewer.
- Inspect package contents with `npm pack`.
- Install the packed tarball into a clean prefix and smoke-test the global
  `architext` binary against a temporary target repository.

## Planned Capabilities

### PDF Export

Goal: export selected Architext views as a portable artifact for review,
archival, or offline sharing.

Open design questions:

- Should PDF export render the existing viewer through a headless browser,
  generate a print-specific static document, or support both?
- Should export scope be active view, all views, selected views, or a report
  bundle?
- How should oversized diagrams, route warnings, clipped content, and hidden
  details be reported?

Acceptance direction:

- PDF export must not require copied viewer files in target repositories.
- Export diagnostics should fail loudly for missing fonts, oversized diagrams,
  clipped content, or route warnings.
- Visual fixtures should cover large and dense diagrams before the feature is
  considered release-ready.

### Release Truth

Target: Architext 1.2.0. Shipped.

Goal: make release posture, scope, blockers, milestones, workstreams, and
historical release trends first-class Architext data.

Direction:

- Add a release index plus one detail JSON file per release so the current
  snapshot is cheap to load and historical releases remain navigable.
- Render a native Release Truth mode inside the existing Architext interface
  rather than copying one-off status page styling.
- Show current target release, posture, progress, blockers, dependencies,
  completed/in-progress/planned/stretch scope, milestones, next actions, and
  last-updated metadata.
- Add historical navigation with a compact trends chart: release date on the X
  axis, counts on the Y axis, and separate lines for features and bug fixes.
- Validate release dependencies, stale status, broken detail-file references,
  and inconsistent summary counts through the normal `validate`/`doctor`
  lifecycle.

See `docs/architecture/RELEASE_TRUTH_PLAN.md`.

### Release Planning

Target: Architext 1.3.0.

Goal: turn roadmap items and ad hoc release work into proposed release scope
without creating a second release model.

Direction:

- Keep Release Truth as the reviewed source of truth; Release Planning should
  author proposals that write into the same release JSON after user review.
- Let maintainers compose a release from existing roadmap items, newly entered
  ad hoc items, blockers, dependencies, milestones, and evidence requirements.
- Surface LLM-made scope recommendations, ordering, priority, and deferral
  rationale before those decisions become Release Truth.
- Preserve a single path from planning to tracking: draft release plan,
  reviewed Release Truth data, then visual tracking and history.

### Schema And Data Migrations

Goal: make schema evolution safe for existing data-only repositories.

- Decide when schema changes require explicit migration commands.
- Keep architecture facts from being rewritten silently.
- Make migration dry-runs show data changes separately from lifecycle metadata
  and instruction updates.

### Source Extraction

Goal: assist teams in maintaining Architext data without pretending generated
architecture facts are automatically authoritative.

- Decide whether source-code extraction should be plugin-based by
  language/ecosystem.
- Treat extraction as draft architecture evidence requiring review.
- Keep validation deterministic and independent of LLM inference.

## Not Roadmap Items

- Hosted SaaS documentation.
- Runtime CDN dependencies.
- Target-repository copied viewer/schema/tool files.
- In-browser editing of architecture JSON.
- Public README instructions for protected package publication operations.
