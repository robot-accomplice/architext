# Architext 1.4.0 Success Criteria

This document defines success for the approved Architext 1.4.0 release scope
before implementation proceeds. Release Truth remains the operational source of
truth; this file clarifies architectural boundaries, acceptance checks, and
non-goals.

## Release Intent

Architext 1.4.0 should make repository-maintained project rules first-class,
continue hardening Release Truth, and add the next diagram/data lifecycle
capabilities without weakening the data-only repository model.

## Required Scope

### Rules Section

Success:

- Rules are repository-owned architecture/project data under
  `docs/architext/data`, loaded through `manifest.json`, validated by
  package-owned schemas, and rendered by a native top-level Rules view.
- Rules are ranked by explicit criticality and ordering, not alphabetical order
  or creation time alone.
- Rules can distinguish maintainer-authored and agent-authored entries, but both
  use the same rule item shape.
- Rules support edit/delete protection per entry. Protected entries remain
  visible and selectable but cannot be deleted or reordered by the normal edit
  flow.
- Unprotected entries can be reordered within their priority group without
  changing protected ordering.
- The Rules UI follows the Release Truth pattern: left browse list, central
  ranked truth view/editor, right detail pane.
- Browser rule edits use the same structured JSON write path as other
  repository-owned Architext data. A failed validation must not leave
  `rules.json` in the invalid candidate state.
- Rules criticality labels do not reuse Release Truth warning/failure colors;
  criticality is priority, not release health.
- Detail panes keep consistent readable insets for headers and body content,
  including when the selected content is a Release Truth item.
- Rule categories are user-defined classifications such as Architecture,
  Development, Design, Release, or any project-specific grouping the maintainer
  chooses. The left browse pane shows those categories instead of duplicating
  the ranked rule list.
- Adding a category creates the first rule draft in that category because
  categories are derived from rule data rather than maintained as a separate
  taxonomy file.

Non-goals:

- Do not turn Rules into a general wiki.
- Do not store rules only in generated prose instructions.
- Do not make browser editing the only path for maintaining rules JSON.

Proof:

- Validation rejects duplicate rule IDs and invalid protection/order metadata.
- Tests cover ordering, protected entry behavior, manifest loading, and direct
  hash navigation.
- Agent instructions mention `docs/architext/data/rules.json` as a maintained
  source of truth when present.

### Draft Release Plan Approval

Success:

- Saved draft release plans remain approvable without rebuilding a transient
  preview payload.
- Approval promotes a saved draft to `planned`, retargets roadmap items, and
  refreshes release index counts.
- The UI keeps approval controls available for draft releases.

Proof:

- API and UI tests cover persisted-draft approval.
- Release Truth for 1.4.0 marks this correction complete after verification.

### Release Truth Summary Visibility

Success:

- Release Path item rows show each item title, concise summary, state, and
  compact metadata.
- Longer rationale, blockers, dependencies, evidence, and next actions remain
  in the details pane.

Proof:

- Static UI tests guard summary rendering.
- Browser smoke confirms summaries are visible in View Truth mode.

### Non-Destructive Data Refresh

Success:

- Data watching never replaces in-progress browser edits without user action.
- Valid file refreshes are applied atomically to the in-memory model only when
  the active editing surface is clean.
- Invalid or mid-write data continues to preserve the last known good model.
- JSON writes use structured object serialization and same-directory temporary
  files before replacing the target file.

Proof:

- UI tests cover release-planning dirty state guarding auto-refresh.
- Runtime tests cover stable JSON writes without leaked temporary files.
- Browser smoke demonstrates editing a release plan while data-watch notices do
  not clear pending input.

### Approved/In-Process Plan Extension

Success:

- Adding items to an existing approved or implementing release plan preserves
  the status and implementation metadata of existing release items.
- Release Planning can extend the selected release without rebuilding already
  completed items as fresh planned scope.
- Roadmap updates remain traceable without duplicating the same logical item.

Proof:

- Domain/API tests cover extending an existing release with a complete item and
  confirm the complete item remains complete.

### Release Item State Language

Success:

- Release Truth never labels unblocked incomplete items as `Clear`.
- Incomplete items without blockers are labeled `Not Blocked`.
- Blocked, complete, deferred, and cut items retain their existing meaning and
  color semantics.

Proof:

- Presentation tests cover the shared state-label function used by Release
  Path, Kanban, milestone rows, and details.

### Workflow Diagrams

Success:

- Workflow diagrams become a first-class Flows projection for ordered work/use
  case paths.
- Workflow rendering reuses the shared route planning and step-pill mechanisms.
- Workflow data does not duplicate existing flow or view facts when a projection
  can derive them.
- Workflow appears as a selectable Flows projection, not as a separate top-level
  mode or a forked renderer.
- The Flows browse panel exposes compatible view projections so a maintainer can
  switch between system map, workflow, and dataflow views without leaving the
  selected flow context.

Non-goals:

- Do not create a second routing implementation.
- Do not introduce workflow-specific line drawing rules.

Proof:

- View-selection tests prove `workflow` belongs to Flows.
- Validation accepts `workflow` view data.
- The self Architext dataset includes one workflow projection over existing
  nodes and flows.

### Schema And Data Migrations

Success:

- Architext provides an explicit migration path for schema/data contract changes
  in data-only repositories.
- Migration commands support dry-run output, validation before/after, and clear
  reporting of changed files.
- Purely additive schema changes may remain minor releases; breaking schema
  changes require a major semver release and a deterministic migration.
- Schema migration planning is a domain concern, not string formatting embedded
  in the CLI adapter.
- `doctor` and `sync` report pending schema migrations through the same repair
  pipeline that applies deterministic lifecycle repairs.

Proof:

- Domain tests cover no-op, additive, and breaking schema migration plans.
- CLI tests cover dry-run and applied schema migration output for stale target
  metadata.

### PDF Export

Success:

- Export produces a portable artifact for the active view with diagnostics when
  export cannot safely complete.
- Export preserves the visible view rather than inventing a separate layout.
- The first export path may use the browser's native print/save-as-PDF workflow,
  but it must be surfaced as an Architext control and keep the active diagram as
  the printed artifact.

Proof:

- UI code exposes a PDF export control through shared diagram controls.
- Print styles hide non-artifact chrome while preserving the active diagram
  canvas, header, and selected step context.

### Source Extraction Drafts

Success:

- Source extraction can propose draft architecture-data changes for review.
- Proposed changes are never applied silently and never replace validation.
- The first source extraction path may be a dedicated `architext prompt` mode
  that asks an agent to inspect source files and return a reviewable draft plan
  before any JSON edits are made.

Proof:

- CLI prompt tests cover the source extraction mode and its no-silent-apply
  guidance.

## Release Checks

- `npm test`
- `npm run build`
- `npm run validate`
- Browser smoke for Release Truth and Rules once implemented.
