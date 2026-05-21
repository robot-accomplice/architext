# Architext 1.4.1 Success Criteria

## Release Intent

Architext 1.4.1 is a patch release for three corrections:

- Rules editing must present action controls as part of the edit form, not as
  detached header controls.
- Release verification must include an automated browser-level UAT harness that
  exercises package-owned viewer behavior against repository-owned data.
- Model-specific instruction files should be migratable into the central,
  model-agnostic Rules source of truth.

This release must not introduce schema changes. It may add release-gate
infrastructure and UI corrections.

## Rules Editor Corrections

The Rules editor should behave like one coherent form:

- Move, delete, cancel, and save actions sit below the editable fields and
  summary text.
- Save is visually primary and visually stronger than all other rule form
  actions. Delete is visually destructive. Move actions are secondary. Cancel
  is neutral.
- Unsaved draft rules cannot be moved or deleted before they have a persisted
  rule identity.
- Protected rules still honor `protection.edit` and `protection.delete`.
- Failed writes report a JSON API error instead of surfacing an invalid server
  response caused by an app-shell fallback.

## Shared Unsaved Editor Guard

Editor navigation should use one shared guard:

- Dirty Rules and Release Planning edits block browser unload.
- Dirty Rules and Release Planning edits ask for confirmation before internal
  navigation changes the active mode, release, rule, category, view, flow, or
  selected detail.
- Confirmed navigation may discard edits; cancelled navigation leaves the user
  in the current editor.
- Saved or cancelled edits clear the dirty state.

## Agent Instruction Rule Migration

Rules migration should make Architext the durable source of project rules while
preserving safe agent entry points:

- `doctor` and `sync` detect rule-bearing `AGENTS.md`, `CLAUDE.md`, Cursor
  rule files, and legacy `.cursorrules` files.
- Dry-run output reports candidate rules, files that would be rewritten, and
  any ambiguous content that must remain untouched.
- Migration requires explicit confirmation before rewriting model-specific
  instruction files.
- Migrated rules are added to `docs/architext/data/rules.json` without
  duplicating existing rules.
- Rewritten instruction files point agents to the Architext Rules data and
  validation flow instead of carrying divergent project rules.
- Unrelated user/project instructions are preserved unless the user explicitly
  authorizes replacement.
- The migration remains deterministic and local; it does not depend on a
  specific LLM provider or external project repository.

## Automated UAT Harness

The verification harness should prove maintainer-visible workflows without
depending on external repositories:

- It starts the package-owned viewer server against a temporary copy of this
  repository's Architext data.
- It loads the viewer in a headless browser.
- It fails on page errors and unexpected browser console errors.
- It exercises write-backed behavior only through the UI controls a maintainer
  would use. API route behavior remains covered by unit and integration tests.
- It covers top-level navigation, panel controls, diagram controls, Rules
  editing, and Release Truth planning controls.
- It exits cleanly and tears down browser, server, and temporary data state.
- It is available as `npm run test:uat`.
- It runs in CI and in the release gate before packaging.

## Verification

Before release:

- `npm test`
- `npm run validate`
- `npm run build`
- `npm run test:uat`
- `npm run release:check`
- `just release-check`, which also refreshes README-facing screenshots and
  audits release-facing documentation markers.
- README text, badges, and screenshot set reflect the release candidate UI and
  feature set.
- Known public project-site references to Architext are checked locally, when a
  maintainer has that site checkout available, for stale version numbers and
  feature descriptions. This check is part of the human release ceremony, not a
  formal Architext repository dependency.
