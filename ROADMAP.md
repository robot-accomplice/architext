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

Goal: make releasability verifiable without depending on local sibling
repositories.

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

## Local Litmus Repositories

Roboticus and Aegis are useful local stress tests for complex diagrams, but
they must not become formal lifecycle dependencies. CI and package release
gates must remain self-contained.

## Not Roadmap Items

- Hosted SaaS documentation.
- Runtime CDN dependencies.
- Target-repository copied viewer/schema/tool files.
- In-browser editing of architecture JSON.
- Public README instructions for protected package publication operations.
