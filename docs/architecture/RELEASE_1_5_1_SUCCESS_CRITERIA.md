# Architext 1.5.1 Audit Remediation Success Criteria

Architext 1.5.1 completes the remaining audit remediation work after the 1.5.0
diagram and skill release. The release must preserve the package-source and
target-data split: package-owned viewer source, schemas, validator, Vite config,
and built package assets live under `viewer/`; target-owned architecture state
remains under `docs/architext/data`.

## Required Outcomes

- The Architext package repository no longer uses `docs/architext` for
  package-owned source files, so `architext sync` cleanup for copied installs
  cannot delete source required to build or publish Architext itself.
- Remaining high-priority audit remediation is complete or explicitly replaced
  by a documented systemic alternative.
- Medium-priority findings have focused tests or documented architectural
  closure where code change is not the correct fix.
- Low-priority cleanups are addressed when they are local to touched files and do
  not expand the release beyond audit remediation.
- Package, CLI, viewer, UAT, routing fitness, and packed-CLI smoke checks pass.

## Verification

- `npm run validate`
- `npm run check:install-docs`
- `npm test`
- `npm run build`
- `npm run test:benchmark`
- `npm run test:uat`
- `npm pack --dry-run --json`
- `npm run smoke:pack`

