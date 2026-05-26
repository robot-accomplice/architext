# HTTP Write Handler Locking

Architext has one target-scoped write lock for repository-owned data. The lock
must be enforced at the HTTP write adapter boundary, not only in the CLI server
that happens to call those adapters today.

## Architecture

Disk-writing HTTP adapters accept a `withTargetWriteLock(target, callback)`
dependency. Mutating operations run all read-modify-write work inside that
callback, including validation and rollback. This keeps direct adapter imports,
tests, and future route wiring under the same serialization contract as the
local serve process.

The CLI server remains responsible for choosing the concrete lock
implementation. It passes the target write lock into the release-planning and
rules adapters instead of wrapping those calls separately.

Read-only preview operations do not take the write lock. They may read target
data and build proposed output, but they must not write files, validate the
target, or block unrelated writes.

## Protected Write Paths

- Rules mutations: `POST /api/rules`
- Release plan approvals and draft saves: `POST /api/release-plans`

Doctor repair and sync repair still own their write orchestration in the CLI
serve layer, so their existing target locks stay at that boundary until those
flows are extracted behind HTTP adapters.

## Verification

- Direct rules adapter calls invoke the supplied target lock before writing.
- Direct release-plan approve and save-draft calls invoke the supplied target
  lock before reading and writing release data.
- Release-plan preview calls do not invoke the target lock and do not write.
- The CLI route wrappers pass the concrete target lock into the adapters rather
  than nesting another lock around them.
