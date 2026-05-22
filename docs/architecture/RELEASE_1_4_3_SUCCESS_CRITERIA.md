# Architext 1.4.3 Success Criteria

## Release Intent

Architext 1.4.3 is a patch release for local serve lifecycle controls. The
goal is to make `architext serve` easier to use from a normal terminal without
turning local viewer servers into unmanaged background processes.

This release must preserve the current foreground serve default for
compatibility. Background behavior is opt-in.

## Serve Lifecycle

`architext serve` should support explicit process ownership modes:

- The default `architext serve` remains foreground and blocks the terminal until
  interrupted.
- `--background` starts the local viewer server detached from the invoking
  terminal, waits until the viewer is reachable, prints the URL, and returns
  control to the shell.
- Background servers are recorded in runtime state with target path, host, port,
  pid, URL, start time, and log path.
- `--status` reports the recorded server for the target and verifies whether it
  is still reachable.
- `--stop` stops the recorded server for the target and removes stale runtime
  state.
- Stale runtime state is detected and cleaned up instead of blocking new serve
  attempts indefinitely.

## Browser Launch

Browser launch should be explicit and cross-platform:

- `--open` launches the system browser after the viewer is reachable.
- `--no-open` suppresses browser launch when combined with aliases or future
  defaults.
- Browser launch uses platform-native commands and remains best-effort. If the
  browser launch fails, the CLI prints the URL and reports the launch failure
  without killing an otherwise healthy server.
- Printed serve output always includes a plain URL that terminal applications
  can auto-link.
- When stdout is an interactive terminal, Architext may also emit an OSC 8
  hyperlink, but the plain URL remains the compatibility baseline.

## Options

The serve command accepts:

- `--foreground` to force current blocking behavior.
- `--background` to start detached and return control.
- `--open` and `--no-open` to control system browser launch.
- `--host <host>` and `--port <port>` to control the bind address.
- `--status` to inspect a recorded background server.
- `--stop` to stop a recorded background server.

Conflicting options fail loudly. Examples include `--foreground --background`
and `--status --stop`.

## CLI Help and Documentation

The CLI help and README should make the serve lifecycle discoverable without
requiring source inspection:

- `architext --help` lists every serve lifecycle switch with default host and
  port values.
- Help examples include foreground serve, background serve, browser launch,
  status, stop, and custom host/port usage.
- README documentation explains that plain `serve` remains the compatibility
  foreground path.
- README documentation explains that background servers are local-only runtime
  processes and should be inspected with `--status` or stopped with `--stop`.

## Out of Scope

- Hosted or remote viewer service.
- Authentication for the local viewer.
- Changing the default `serve` behavior to background.
- Publishing the viewer to a remote URL.

## Verification

Before release:

- CLI parser tests cover serve lifecycle switches and conflicts.
- CLI help tests cover every serve lifecycle switch and examples for
  foreground, background, browser launch, status, stop, and custom host/port.
- Serve lifecycle unit tests cover foreground startup, background metadata,
  stale runtime cleanup, status, and stop behavior.
- Browser opener tests cover macOS, Windows, Linux, and unsupported platforms.
- Existing serve handler tests continue to pass.
- README documents foreground, background, browser launch, status, and stop
  usage.
- `npm test`
- `npm run validate`
