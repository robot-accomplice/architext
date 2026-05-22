# Architext 1.4.4 Success Criteria

Architext 1.4.4 is a patch release for resilient local recovery when target
architecture data is invalid, incomplete, or actively being written.

## Architecture

The browser viewer must stay useful when target data is problematic. Invalid
data is a recoverable operating mode, not a terminal application failure.

The release has four coupled workstreams:

- **Degraded data shell:** the viewer loads a basic navigation and recovery
  surface even when the architecture model cannot be constructed.
- **Browser repair actions:** served viewers can request status, doctor dry-run,
  doctor apply, and sync repair flows through local HTTP APIs instead of only
  telling users to leave the UI and run terminal commands.
- **Write-lock discipline:** every mutating path that can rewrite Architext
  target files waits for active writes to settle and holds a shared lock while
  applying changes.
- **Release automation planning:** document the npmjs trusted-publishing path
  without changing the existing package contract in a patch release.

## Success Criteria

### Degraded Data Shell

- Initial data-load or reference-validation failure renders a recovery screen
  inside the application chrome instead of replacing the whole viewer with a
  dead-end error page.
- The recovery screen shows the validation or load error in a bounded, readable
  diagnostic area hidden behind a Details disclosure by default.
- The viewer preserves the last known good model during live invalid data
  events and exposes the current invalid state without forcing a destructive
  refresh.
- Live invalid data events open a centered modal dialog with Wait and Now
  choices, replacing the previous browser-native confirm and avoiding a
  dismissible banner for a state that requires an explicit decision.
- Choosing Wait dismisses the dialog, keeps the last known good model
  interactive, and relies on background validation to refresh when the data is
  valid again.
- The degraded shell does not pretend the model is healthy; diagram, release,
  planning, and rules views that depend on valid data must be disabled or
  clearly scoped to the last known good model.
- Browser mutation controls that write target data must not allow overlapping
  requests from repeated clicks while a prior mutation is still pending.

### Browser Repair Actions

- The served viewer exposes local recovery actions for:
  - status inspection;
  - doctor dry-run;
  - deterministic doctor repair apply;
  - sync repair using default noninteractive choices.
- Browser repair actions reuse the same domain and CLI repair derivation code as
  `architext doctor` and `architext sync`; they must not grow a separate repair
  model in the UI.
- API responses include structured success/failure state, command output, repair
  summaries, and whether the viewer should reload data after completion.
- Failed repair attempts remain actionable: the UI shows the failure output and
  keeps recovery controls available.
- Recovery action controls use the same uppercase command style as the rest of
  the viewer and leave a persistent visible result after completion.
- Browser-triggered sync must not create git branches, rewrite unrelated user
  files, or prompt interactively.

### Write-Lock Discipline

- All Architext mutating paths share one target-scoped write lock:
  CLI doctor apply, CLI sync writes, rules edits, release planning writes, and
  browser-triggered recovery actions.
- Before acquiring the lock, mutating tasks wait for active JSON writes to settle
  so they do not read half-written data or race external editors.
- While a lock is held, competing mutating tasks wait up to a bounded timeout
  and then fail loudly with a non-corrupting error.
- Stale locks are detectable and recoverable without forcing users to manually
  delete unknown files.
- Lock state is not represented as architecture data and must not be committed
  to target repositories.

### Release Automation Planning

- The 1.4.4 release truth records the plan for removing manual npmjs
  publication steps.
- The plan must preserve the existing npmjs package identity,
  `@robotaccomplice/architext`; changing that package name is out of scope for
  a patch release.
- GitHub Packages setup remains out of scope while the namespace mismatch exists:
  GitHub Packages scopes packages by the GitHub account or organization owner,
  while this repository currently lives under `robot-accomplice` and the npmjs
  package scope is `robotaccomplice`.
- Publishing a second package identity or moving the repository owner are
  non-starters for this release.

## Non-Goals

- Do not make the browser execute arbitrary shell commands.
- Do not add a general terminal emulator to the viewer.
- Do not silently overwrite invalid target data with starter data.
- Do not make repair APIs available from static `file://` builds.
- Do not bypass validation after repairs; successful repairs must end with a
  validation pass or an explicit validation failure.
- Do not change the public npm package name or install instructions in 1.4.4.

## Verification

- Unit coverage for write-lock acquisition, wait, timeout, stale-lock recovery,
  and cleanup.
- HTTP API coverage for status, doctor dry-run, doctor apply, sync repair,
  validation failure, and lock contention.
- UI/domain coverage for initial invalid data loading into the degraded shell
  and live invalid data preserving the last known good model.
- CLI regression coverage proving doctor and sync still behave the same from the
  terminal while using the shared lock.
- Release gate: `npm test`, `npm run validate`, `npm run build`, UAT, package
  dry-run, and packed CLI smoke.
