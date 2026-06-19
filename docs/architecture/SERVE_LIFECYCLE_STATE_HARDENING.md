# Serve Lifecycle State Hardening

Serve state lives in the OS temporary directory. That state is local runtime
metadata, not trusted project data.

## Contract

- Stored serve URLs are reachable only when they resolve to loopback hosts.
- Foreground and background serve processes both record live instance state so
  discovery commands describe what is actually running.
- Background startup for a target uses a target-scoped runtime mutex so two
  `serve --background` calls do not race through check-then-spawn state.
- Log file descriptors opened for child process stdio are closed even if spawn
  fails.
- `--port 0` is an explicit ephemeral-port request for tests and local
  automation. The server must record and print the actual OS-assigned port, not
  `0`, so callers never pre-bind, close, and later reuse a guessed free port.

The serve lifecycle may clean stale runtime records, but it must not use
untrusted runtime state to fetch arbitrary URLs or spawn duplicate background
servers for the same target.

Foreground records do not have log files because their output belongs to the
terminal that owns the blocking process. They still carry target path, host,
port, PID, URL, start time, and mode so `architext --list` can find them while
they are alive.

## Verification

- Non-loopback stored URLs are considered stale and are not fetched.
- `architext --list` discovers a live foreground `architext serve` process.
- Concurrent background startup attempts for one target serialize through the
  runtime mutex.
- A spawn failure after opening the log file closes the file descriptor.
- Serve lifecycle tests use `--port 0` or a currently-bound blocker socket for
  port allocation; they do not use close-then-reuse free-port probes.
