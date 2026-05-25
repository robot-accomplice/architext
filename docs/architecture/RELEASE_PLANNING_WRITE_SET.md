# Release Planning Write Set

Release Planning approval writes a coordinated set of repository-owned JSON
files:

- the release detail file for the planned version
- the release index
- the roadmap
- any prior release detail files that receive deferred-transfer markers

These files form one logical write set. A failed post-write validation must not
leave only part of that set on disk.

## Write Contract

The adapter must capture the previous state of every file before its first write
in a request. If target validation fails after the write set is applied, the
adapter restores every captured file:

- existing files are restored to their previous JSON content
- files created by the failed request are removed

Dry-run requests do not write files and do not need rollback state. Reference
validation still runs before writes so obviously invalid plans fail before the
repository is touched.

## Verification

- Approval still writes and validates a complete plan on success.
- Save-draft still writes only draft release state and leaves roadmap targets
  unchanged.
- If target validation rejects an approval, release detail, release index,
  roadmap, and deferred-transfer source files match their pre-request state.
