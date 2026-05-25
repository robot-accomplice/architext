# Write Lock Recovery

Architext uses a target-scoped directory lock at
`docs/architext/.architext-write.lock/` to serialize repository-owned data
writes. Creating the lock directory is the ownership boundary: only the process
that successfully creates that directory may write while the lock is held.

## Stale Lock Reclamation

Stale lock recovery must also be atomic. A process must not delete the lock path
directly after observing that it is stale, because another process can observe
the same stale path at the same time.

The recovery sequence is:

1. Determine that the existing lock is stale from its owner metadata and age.
2. Atomically create a fixed reclaim marker directory inside the lock directory.
   Only one contender can create that marker.
3. Re-check that the marked lock directory is still stale. This second check is
   required because another process may have acquired a fresh lock after the
   first observation.
4. Delete the lock directory only if the marked directory is still stale.
   Otherwise, remove the reclaim marker and wait for the active owner.
4. Retry normal lock acquisition with `mkdir`.

Only one contender can mark a lock for reclamation. Other contenders either
observe that reclamation is already in progress, observe that the original path
disappeared, or find the replacement lock created by the winning contender. They
must return to the normal acquisition loop instead of assuming ownership.

## Verification

- A stale lock can still be recovered without manual cleanup.
- Concurrent contenders after a stale lock execute their critical sections one
  at a time.
- Reclaimed lock directories are temporary and are removed after recovery.
