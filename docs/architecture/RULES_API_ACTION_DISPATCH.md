# Rules API Action Dispatch

Rules mutations are a small command API over the rules document. The adapter is
responsible for decoding request intent before any domain mutation runs.

## Contract

- `action` is allowlisted before dispatch.
- Missing `action` remains a legacy `update` request.
- Unknown actions fail before writing the rules document.
- Each allowed action maps to exactly one domain command.

This keeps routing deterministic in the adapter and leaves rule semantics in the
domain module.

## Verification

- Unknown actions reject without changing the stored rules document.
- Legacy update requests without `action` still upsert the submitted rule.
