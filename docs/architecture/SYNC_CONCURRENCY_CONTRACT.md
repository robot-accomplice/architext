# Sync Concurrency Contract

`architext sync` can run from the CLI and from the local viewer repair endpoint.
Both paths may mutate repository-owned files. The implementation must keep
request-local behavior local and shared write behavior serialized.

## Output Capture

Sync output capture must be request-scoped. The server must not monkey-patch
global `console.log` to capture output for `/api/sync-repair`, because
overlapping requests can capture each other's messages or restore the global
logger out of order.

Sync accepts a logger sink. CLI callers use the process console. HTTP repair
callers provide a per-request collector.

## Write Lock Scope

For non-dry-run sync, the target write lock covers every sync mutation,
including branch creation. Prompting and status collection may happen before the
lock, but once the user has selected changes, the mutation phase runs under the
lock.

## Verification

- HTTP sync repair captures output without replacing `console.log`.
- Sync still prints normal CLI output when no custom logger is supplied.
- Non-dry-run branch creation happens inside the same locked mutation phase as
  data and instruction writes.
