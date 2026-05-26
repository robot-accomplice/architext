# Architext 1.4.11 Success Criteria

This document defines success for the Architext 1.4.11 patch release. The
release packages the post-1.4.10 agent-instruction and serve lifecycle
corrections.

## Architecture

Agent-facing Architext instructions must make flow and sequence diagram quality
harder to misunderstand. Flow diagrams must not leave orphaned nodes, labels, or
markers; every rendered node, edge, marker, and label must be traceable to a
selected flow, supporting relationship, or explicit context relationship.
Disconnected context must be removed, connected, or split into a separate view
instead of left for the reader to interpret. Sequence diagrams must make return
paths explicit and use loops, retries, optional branches, and transaction or
consistency blocks when those structures govern outbound and return messages.

Serve startup treats the configured port as the preferred starting port. If the
preferred loopback port is already occupied, foreground and background serve
advance to the next available loopback port, record the effective port, and
print the actual URL. Recorded lifecycle commands still use the recorded host
and port for existing instances.

## Documentation Requirements

- Architecture documentation records sequence return-path and flow orphan
  invariants.
- Managed agent instructions and prompt output include the diagram-quality
  requirements.
- Public serve documentation explains that `--port` is a preferred starting
  port, not a hard manual recovery burden.
- Release Truth records the patch scope and verification.

## Verification

- CLI prompt tests assert the new model instruction text.
- Managed instruction sync tests assert generated `AGENTS.md` and `CLAUDE.md`
  include the new diagram constraints.
- Serve lifecycle tests assert foreground and background serve advance when the
  preferred port is occupied.
- `npm run release:check` passes before release.

## Out of Scope

- Changing the Architext data schema.
- Adding new authored sequence-diagram JSON structures.
- Changing recorded-instance restart semantics.
