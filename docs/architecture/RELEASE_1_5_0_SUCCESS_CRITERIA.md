# Architext 1.5.0 Success Criteria

This document defines success for the Architext 1.5.0 release. The release
packages diagram semantics, the shareable Architext skill, and audit-hardening
work already completed on the 1.5.0 data schema line. Remaining audit
remediation is explicitly deferred to the next release.

## Architecture

Flow diagrams must keep decisions separate from components. Decision branches
must originate at the decision diamond, carry clear outcome labels, use one
step identity for the decision, and highlight every affected node and branch
when selected. Orthogonal routes must preserve straight east-west connections
between facing surfaces where possible, distribute multiple ports across a
surface, and use line hops for accepted perpendicular crossings.

Sequence diagrams must render lifeline activation bars, explicit return paths,
transaction frames, loop frames, and semantically distinct outbound and return
messages. Return paths remain tied to the outbound interaction they answer.

Architext skill content is package-owned and printable through the CLI so a user
can copy it into any LLM environment without knowing that environment's skill
installation mechanism.

## Documentation Requirements

- Release Truth records the 1.5.0 scope and the audit work deferred to the next
  release.
- The Architext skill documents flow, sequence, Release Truth, and routing
  expectations for model-specific skill creation.
- Architecture notes describe the shared step-route presentation model and
  single-side route centering constraints.
- Public package metadata identifies the artifact as 1.5.0.

## Verification

- `npm run release:check` passes before release.
- Flow display tests assert decision branches share one step identity and stay
  separate from component routes.
- Route-planning tests assert centered single-surface routes, centered side
  endpoints, port distribution, and crossing hops.
- Sequence route-model tests assert outbound/return semantics and activation
  derivation.
- CLI tests assert `architext skill` prints the packaged skill content.

## Deferred

- Complete the remaining superpowers audit remediation in the next release.
