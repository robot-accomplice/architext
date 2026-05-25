# Serve Lifecycle State Hardening

Background serve state lives in the OS temporary directory. That state is local
runtime metadata, not trusted project data.

## Contract

- Stored serve URLs are reachable only when they resolve to loopback hosts.
- Background startup for a target uses a target-scoped runtime mutex so two
  `serve --background` calls do not race through check-then-spawn state.
- Log file descriptors opened for child process stdio are closed even if spawn
  fails.

The serve lifecycle may clean stale runtime records, but it must not use
untrusted runtime state to fetch arbitrary URLs or spawn duplicate background
servers for the same target.

## Verification

- Non-loopback stored URLs are considered stale and are not fetched.
- Concurrent background startup attempts for one target serialize through the
  runtime mutex.
- A spawn failure after opening the log file closes the file descriptor.
