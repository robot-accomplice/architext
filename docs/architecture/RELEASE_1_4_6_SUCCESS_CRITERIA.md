# Architext 1.4.6 Success Criteria

This document defines success for the Architext 1.4.6 patch release before
implementation proceeds. The release is intentionally minute: it clarifies
repository ownership language and removes misleading sample-data framing from
the self-hosted Architext model.

## Architecture

Architext target repositories own architecture facts, lifecycle metadata, and
optional instruction pointers. They must not copy or maintain package-owned
viewer, schema, tool, package, Vite, TypeScript, public asset, README, or
dependency files.

The systemic fix is documentation and CLI contract alignment:

- `architext --help` names the project-owned target files directly.
- `architext --help` names package-owned files that should not be copied into
  target repositories.
- The README and self-model describe the checked-in Architext data as the real
  Architext project model.
- Release Truth records this patch as documentation and help-text correction,
  not new runtime behavior.

## Documentation Requirements

- README uses "Current Architecture Model" for the Architext self-hosted data.
- README describes optional target-owned `AGENTS.md`, `CLAUDE.md`, Cursor rule,
  and `.cursorrules` pointers.
- Architecture data includes Cursor rules and `.cursorrules` in the target
  repository ownership boundary.
- CLI help documents the same ownership boundary.

## Verification

- `node --test test/cli.test.mjs` passes.
- `node tools/architext-adopt.mjs validate .` passes.
- `node tools/architext-adopt.mjs --help` shows the updated ownership text.
- `rg "\\bdemo\\b|\\bDemo\\b" README.md docs/architecture/ARCHITECTURE_PLAN.md docs/architext/data -S`
  returns no matches.

## Out of Scope

- Runtime behavior changes.
- Schema changes.
- Viewer UI changes.
- New install or migration workflows.
