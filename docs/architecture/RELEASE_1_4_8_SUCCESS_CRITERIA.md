# Architext 1.4.8 Success Criteria

This document defines success for the Architext 1.4.8 patch release. The
release closes the audit remediation stack after 1.4.7 and includes the
Release Path usability follow-up.

## Architecture

1.4.8 keeps the existing data schema and CLI shape. The release is a hardening
and presentation release, not a new planning model.

The architectural priorities are:

- enforce the local dashboard trust boundary before any write handler mutates
  target repository files;
- keep multi-file write paths serialized or rollback-safe;
- make concurrency and release-planning safety nets part of the normal test
  suite;
- keep route endpoint selection deterministic and centered for single-surface
  connectors;
- continue reducing `main.tsx` by moving Release Truth presentation concerns
  into focused presentation components;
- let Release Path readers collapse completed schedule groups without losing the
  milestone status, timing, blocker summary, or completion count.

## Documentation Requirements

- Architecture notes describe every new boundary introduced by the release.
- Release Truth records the shipped scope, evidence, and release ceremony
  outputs.
- The release success criteria explicitly include browser-visible Release Path
  behavior, because this release changes reader ergonomics.

## Verification

- `npm run verify` passes.
- `npm run test:uat` passes.
- `npm pack --dry-run` passes.
- `npm run smoke:pack` passes.
- Browser verification confirms Release Path milestone groups can collapse and
  title metadata includes `X/Y complete` without making the title bar visually
  heavier.
- GitHub CI passes for all merged release-scope PRs.

## Out of Scope

- Schema version changes.
- A new release-planning model.
- Persisting Release Path collapse preferences into repository data.
