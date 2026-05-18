# Architext 1.3.0 Success Criteria

This document defines success for the draft Architext 1.3.0 release scope before
implementation begins. Release Truth data remains the release source of truth;
this file is a review aid for clarifying intent, acceptance checks, and
non-goals.

## Release Intent

Architext 1.3.0 is the Release Planning release. It should let a maintainer and
LLM craft a plan for a specific next release from a mixture of cherry-picked
roadmap items and ad hoc manually entered items. The user reviews proposed
priority, ordering, dependency, milestone, evidence, and deferral decisions
before accepted scope is applied into Release Truth.

## Required Scope

### Release Planning Data Contract

Success:

- Release Planning has a documented data contract that maps proposals into the
  existing Release Truth detail file.
- A release plan targets one specific next release version or release window.
- The model has one reviewed source of truth: release detail JSON. Proposal,
  Kanban, path, history, and details views are projections or write flows over
  that source.
- The contract distinguishes reviewed release state from draft proposal state.
- Release items may record source metadata such as `source` (`roadmap` or `ad-hoc`) and `dateAdded`, but roadmap-picked and ad hoc items share the same release item shape. Ad hoc items are simply items that were not already present on the roadmap.
- Deferrals, priority changes, ordering decisions, dependencies, and rationale
  have a visible place in the accepted release data.

Non-goals:

- Do not introduce a separate task database.
- Do not make Release Truth depend on GitHub, Linear, npm, or other network
  services.
- Do not make browser editing the only path for release planning.

Proof:

- Architecture documentation describes the contract.
- Validation rejects broken release dependencies and stale generated summaries.
- Existing release files continue to validate.

### Viewer Refactor Boundaries

Success:

- The viewer has explicit boundaries for app shell, top navigation, side panels,
  canvas header, canvas viewport, diagram renderers, step summary, and detail
  panel.
- React components render already-shaped presentation data where practical
  instead of owning domain policy.
- Browser APIs remain isolated in adapters or hooks.
- Release Truth and future planning views can be added without deepening
  `main.tsx` as a god component.

Non-goals:

- Do not redesign the UI while refactoring.
- Do not rewrite routing behavior as part of this refactor unless a boundary
  must move.
- Do not create abstractions that serve only one unchanged component.

Proof:

- Existing viewer tests pass.
- Browser smoke checks cover mode switching, side-panel collapse, canvas
  containment, details selection, line-style controls, fit behavior, and step
  selection.
- The diff shows smaller, named modules with stable ownership boundaries.

### CLI Lifecycle Refactor Boundaries

Success:

- `tools/architext-adopt.mjs` becomes a thin executable bootstrap.
- `sync`, `doctor`, `validate`, `build`, `serve`, `prompt`, and `clean` execute
  through explicit use cases.
- Filesystem, package runtime files, validation runner, git state, HTTP serving,
  and interactive prompts are ports/adapters rather than implicit global
  behavior.
- `sync` and `doctor` continue to share one repair derivation path.

Non-goals:

- Do not change public command names or target-path semantics.
- Do not create a framework-heavy command bus.
- Do not split lifecycle logic in a way that makes doctor and sync disagree.

Proof:

- CLI tests cover every public command with omitted and explicit target paths.
- Existing package smoke test still installs the packed CLI and runs sync,
  doctor, validate, and version.
- `architext --help` still accurately documents the supported interface.

### Roadmap-To-Release Scope Assembly

Success:

- Release Planning is an edit mode of the Release Truth screen, not a separate
  navigation destination. The release browser remains the release selector in
  both read and edit modes.
- Direct view links such as `#releasetruth` open the matching top-level view so
  maintainers can share a Release Truth entry point without manual navigation.
- The UI presents roadmap items as a selectable list.
- The user checks the roadmap items they want to include.
- The UI provides an `Add new item` action that opens an inline form for an ad hoc item.
- Submitting the ad hoc form adds that item to the candidate list and selects it automatically.
- Ad hoc items appear in the same selectable list as roadmap items; source is metadata, not a separate UI lane.
- When the list is complete, the user saves or creates the release plan.
- The save/create step lets the user set a version number, prefilled from the latest known version, plus an optional theme or title.
- The user can save a release plan as a draft before approval. Draft saves
  persist the proposed release file and release index summary, but do not update
  roadmap targets and do not make the draft the current release.
- Saved drafts remain visible in the release browser as `DRAFT` and can later be
  approved. Approval promotes the draft to `planned`, retargets roadmap items,
  and makes the approved release current.
- Once implementation work begins, the current release moves from `planned` to
  `implementing`; future drafts remain visible but must not replace the current
  release pointer.
- Draft and approved are distinct lifecycle states; a release plan cannot be both
  draft and approved.
- Saved drafts are editable through the same Release Planning screen; editing a
  draft updates the draft, and approving it promotes that draft.
- Release Planning cannot include roadmap items already committed to another
  release unless the item is explicitly deferred. When deferred scope moves into
  a later release, the original release item records `deferredToReleaseId` so the
  prior release can display `deferred to release x.x.x`.
- Candidate scope includes title, kind, status, priority, owner, optional summary,
  rationale, dependencies, workstream, milestone placement, and evidence needs.
- The assembly path makes LLM decisions visible for review before they become
  accepted Release Truth.

Non-goals:

- Do not infer project truth from source code without review.
- Do not silently overwrite existing release facts.
- Do not turn roadmap text into release state without explicit selection and a
  visible proposal step.

Proof:

- A fixture demonstrates roadmap items becoming proposed release items.
- A fixture demonstrates a manually entered ad hoc item becoming a proposed
  release item.
- The proposal includes enough metadata for Release Path, Kanban, details, and
  validation to render consistently.
- Rejected or deferred items remain visible with rationale when accepted into
  Release Truth.

### Approve Release Plan

Success:

- An approved release plan writes the new release detail JSON and regenerates release index summaries and counts.
- Approval also updates the roadmap source so selected roadmap items and newly added ad hoc items remain traceable outside the release file.
- An unapproved or rejected release plan does not mutate release state.
- The write path is previewable before mutation. Preview shows the release detail file to create or replace, generated release index changes, roadmap items that will be retargeted or added, and validation status for the proposed result.
- Approval and preview use the same release-planning use case; the mutating path only adds filesystem writes after the preview result has been built and validated.

Non-goals:

- Do not bypass validation.
- Do not hide generated index changes among architecture fact changes.
- Do not require target repositories to vendor package-owned tooling.

Proof:

- Tests cover accept, reject, preview, generated-index refresh, and validation
  failure cases.
- The local serve API preview path is covered without requiring a fixed browser
  port, so preview-before-mutation remains a deterministic lifecycle guarantee.
- `architext doctor` and `architext sync` report repairable Release Truth drift
  through the existing lifecycle model.

### Release Truth Kanban View

Success:

- Kanban is a projection of Release Truth items grouped by state.
- Columns support planned, ready, in progress, blocked, deferred, stretch, and
  complete states as the data allows.
- Cards remain compact and inspection-friendly: title, status, kind, priority,
  owner, dependencies, blockers, and evidence are visible or available through
  the details pane.
- The details pane avoids redundant state labels and hides empty release item
  sections so missing implementation-time information does not drown out the
  fields that are already known.
- Selecting a card updates the details pane without losing current release
  context.

Non-goals:

- Do not create a second task model.
- Do not make drag-and-drop mutation part of the first version unless proposal
  application already makes writes safe.
- Do not let Kanban replace the Release Path; it is a different projection.

Proof:

- Browser smoke covers column grouping, card selection, details-pane updates,
  blocked-card state, deferred-card state, and return to current release.
- The same item appears consistently in Release Path, Kanban, and details.
- Before the release ceremony, the 1.3.0 Release Truth data reflects actual
  implementation state: completed items are marked complete, active work is
  marked implementing, stretch items stay stretch, and the generated counts no
  longer underreport known completed work.

### C4 Drilldown

Success:

- Clicking a C4 context box navigates to the container-level C4 view scoped
  within that context when a matching view exists.
- Clicking a C4 container navigates to the component-level C4 view scoped within
  that container when a matching view exists.
- Clicking a C4 component navigates to the code-level C4 view scoped within
  that component when a matching view exists.
- C4 drilldown matching is data-driven through explicit view scope metadata,
  not inferred from view names.
- When no child C4 view exists, the viewer explains why instead of silently
  doing nothing; external dependencies and actors receive domain-specific
  explanations.
- The interaction is discoverable but does not clutter diagrams.
- Drilldown preserves normal selection behavior when no child view exists.
- Users can navigate back or otherwise return to the previous C4 context.

Non-goals:

- Do not infer component diagrams that are not represented in data.
- Do not make drilldown depend on node naming conventions alone if explicit view
  metadata is needed.
- Do not break non-C4 node selection.

Proof:

- C4 fixture includes context, container, and component drilldown.
- Tests cover matching child view, no child view, and return navigation.
- Browser smoke verifies the click behavior visually.

### Validate Release Plan Data

Success:

- Validation covers planned release item references, roadmap/ad hoc item origins, release item dependencies,
  workstream membership, milestone links, blocker links, generated counts, and
  stale release index summaries.
- Doctor reports repairable release-planning drift without inventing facts.
- Invalid planning data fails loudly before the viewer replaces known-good
  state.

Non-goals:

- Do not make validation depend on network state.
- Do not use LLM judgment for deterministic checks.
- Do not auto-repair architecture or release facts without explicit review.

Proof:

- Unit tests cover missing references, stale counts, duplicate IDs, invalid
  statuses, and repairable generated-index drift.
- `npm run validate` passes for Architext self-data.

### Release Planning LLM Instructions

Success:

- Managed LLM instructions explain how to draft next-release plans, record roadmap/ad hoc item origins, and maintain Release Truth after review.
- Instructions require agents to surface priority, ordering, dependency, evidence, and deferral choices instead of hiding them in chat history.
- Instructions preserve package-owned vs target-owned boundaries.
- Public docs do not expose protected publication process details.

Non-goals:

- Do not instruct agents to edit package-owned viewer/schema/tool files inside
  target repositories.
- Do not describe private npm publication operations in the README.
- Do not let planning prompts replace validation.

Proof:

- CLI prompt output includes next-release planning rules.
- Managed `AGENTS.md` and `CLAUDE.md` sections include the new LLM instructions.
- Tests assert the managed instruction text.

### Routing Correctness Pass

Success:

- Every release includes concrete routing improvement work until the remaining
  issues are solved or no longer worth further complexity.
- The 1.3.0 pass targets dense Deployment and Data/Risks overlaps and any
  regressions introduced by refactoring.
- Routing changes remain systemic: route indexing, candidate generation,
  scoring, ports, geometry, labels, or rendering, not view-specific patches.

Non-goals:

- Do not optimize before correctness.
- Do not add one-off route exceptions for a single project fixture.
- Do not mix orthogonal and spline semantics.

Proof:

- Routing fitness tests include dense Deployment/Data-Risks fixtures.
- Orthogonal, spline, and straight line styles either render readable routes or
  report failures clearly.
- Browser smoke checks include at least one dense diagram.

### CI And Release Gate Hardening

Success:

- CI supports the modified Gitflow process: PRs to `develop`, aggregation on
  `develop`, release PRs to `main`, and hotfix backmerges through PRs.
- The release gate uses only this repository's fixtures and package contents.
- The gate runs unit tests, routing/C4 fitness tests, validation, build,
  package dry-run, and packed global CLI smoke tests.

Non-goals:

- Do not require external local projects for formal lifecycle checks.
- Do not rely on trusted publishing until it is actually configured.
- Do not put protected publication operations in public README content.

Proof:

- CI passes on PRs and on pushes to `develop` and `main`.
- `npm run release:check` passes locally and includes the same packed global CLI
  smoke path used by CI.
- Packed CLI smoke test runs against a temporary target repository.

## Planned Scope

### Data File Watching And Auto Refresh

Success:

- `architext serve [path]` watches the selected target repository's `docs/architext/data/**/*.json` files.
- Watching is write-aware and does not refresh mid-write.
- After a successful validation pass, the viewer refreshes without restarting the local server.
- On validation failure, the viewer keeps the last known good model and prompts the user with a warning such as: `The JSON data was updated and left in an invalid state. Refresh anyway?`
- Release data, architecture data, and manifest changes use the same reload path.

Non-goals:

- Do not add target-repo watcher scripts or dependencies.
- Do not refresh from partially written invalid JSON.
- Do not make watching required for static builds.

Proof:

- Tests cover successful refresh, invalid JSON, schema failure, and manifest
  file changes.
- Browser smoke demonstrates a Release Truth data edit refreshing the UI.

### Release Planning Visual Smoke Coverage

Success:

- Browser smoke covers the full Release Planning interface because the UI is intentionally simple.
- Coverage includes roadmap item selection, inline ad hoc item creation, automatic selection after ad hoc submit, version prefill, optional title/theme entry, save/create, Release Truth update, Kanban projection, details pane, history, current release return, side panel collapse, and validation error display.
- Screenshots are useful for human inspection and do not depend on external projects.

Non-goals:

- Do not use screenshots as the only test assertion.
- Do not formalize local litmus repositories as CI dependencies.

Proof:

- Smoke tests run against Architext self-data.
- Failures identify the broken view or interaction.

## Stretch Scope

### Draft Management Follow-Up

Success:

- Drafts can be deleted or explicitly promoted without recreating the plan.
- The UI exposes draft age and last update source.
- Draft management does not create a second authoritative planning model.

Non-goals:

- Do not add collaboration semantics in the first draft feature.

Proof:

- If included, tests cover inspect, promote, reject, and delete draft.

### PDF Export

Success:

- Users can export the active Architext view as a portable PDF artifact.
- Export diagnostics report missing fonts, oversized diagrams, clipped content,
  and route warnings.
- Export does not require copied viewer files in target repositories.

Non-goals:

- Do not build a report designer or multi-view report bundle.
- Do not silently crop diagrams.
- Do not make PDF export block the release if the core Release Planning scope is
  complete and this stretch goal is not.

Proof:

- Visual fixture covers a large/dense diagram export.
- Export command fails loudly or reports diagnostics when fidelity is not
  acceptable.

### Source Extraction Drafts

This stretch item means Architext may later inspect source files to produce suggested architecture-data changes. Those suggestions would be drafts only, requiring human review before they become Architext JSON. The first useful extraction target is not defined for 1.3.0, so this item remains TBD stretch scope.

## Explicitly Out Of Scope

### Hosted Release Planning Service

Architext remains local-first and repository-owned. A hosted planning service is
not part of 1.3.0.
