# Architext 1.4.5 Success Criteria

This document defines success for the Architext 1.4.5 patch release before
implementation begins. The release is limited to local serve instance
management unless Release Truth is explicitly extended.

## Architecture

Architext 1.4.5 improves the local serve lifecycle contract introduced in
1.4.3. The current lifecycle stores one serve process record per target
repository, keyed by a target-path hash in the local runtime directory. That is
enough when the user already remembers the target repository. It is not enough
when multiple local viewers are running and the user needs to discover or stop a
specific instance.

The systemic fix is to make the runtime registry discoverable and addressable:

- `architext --list` lists all recorded serve instances without
  requiring a target path.
- Listed instances include a stable instance id, target path, PID, URL, mode,
  start time, liveness state, and log path when the process has one.
- Stale records are detected with the same PID and URL reachability checks used
  by target-scoped `serve --status`.
- Users can stop a specific listed instance by id rather than by remembering
  the target path.
- Users can refresh a running background instance after upgrading Architext so
  the target is synced with the current package and the server process is
  restarted with the currently installed CLI instead of continuing to run the
  previous package version.
- Users can ask Architext to check the npm registry for a newer package version,
  install it after confirmation, and then refresh selected or all running
  background instances with the newly installed CLI.
- Existing target-scoped commands keep their behavior:
  - `architext serve [path] --status`
  - `architext serve [path] --stop`
  - `architext serve [path] --background`
- The top-level `--list` command is read-only except for optional stale-record
  cleanup. It must not start, stop, or mutate target repositories.

## CLI Contract

Required command surface:

```sh
architext --list
architext serve --list
architext serve --stop --instance <id>
architext serve --restart --instance <id>
architext serve [path] --restart
architext --check-updates
```

`architext --list` and `architext serve --list` are equivalent. They print a
human-readable table by default. If `--json` is provided, they print structured
JSON suitable for scripts.

`--instance <id>` is valid only with serve lifecycle status/stop operations.
The id is the registry id shown by list output. Unknown ids fail loudly and list
the known ids, if any.

`--restart`, `--refresh`, and `--update` are equivalent. They first run the
same target sync operation a user would otherwise run manually after upgrading
Architext, then stop the existing background instance and start it again using
the current Architext CLI entry point. Restart preserves the recorded target,
host, and port. It may be targeted by path or by `--instance <id>`. Restart does
not apply to foreground serve processes, because the user already controls that
terminal process directly.

Refresh must sync before stopping the existing server. If sync fails, the old
background server remains running and the command fails loudly with the sync
error. The refresh path must not create git branches; it uses the lifecycle
sync machinery with non-interactive defaults appropriate for an explicit
refresh command.

`--check-updates` is a top-level lifecycle command. It compares the current
Architext package version to the latest published npm version. If a newer
version exists, it asks before installing the package. After a successful
install, it lists running background instances and asks whether to refresh none,
one, or all. The post-install refresh must invoke the newly installed `architext`
binary rather than relying on the old in-memory process that performed the
update check.

The update check is intentionally separate from `--update`. In 1.4.5,
`--update` remains a serve-instance refresh alias for users who already upgraded
the package; `--check-updates` owns package-version discovery and installation.

## Documentation Requirements

- README serve lifecycle documentation explains global list and instance-stop
  usage.
- CLI help documents `--list`, `--instance <id>`, and the restart aliases.
- CLI help documents `--check-updates` and distinguishes package updates from
  serve-instance refresh.
- Release Truth records 1.4.5 scope and closes 1.4.4 as shipped.

## Verification

- Parser tests cover top-level `architext --list`, `serve --list`, and
  conflicting serve lifecycle flags.
- Serve lifecycle tests cover:
  - multiple background instances listed together,
  - stale instance filtering or cleanup,
  - stopping one instance by id while leaving another running,
  - refreshing one instance by id runs sync, then restarts on the same target,
    host, and port,
  - refresh failure leaves the existing instance running,
  - target-scoped restart for the current repository,
  - update-check reports current when npm has no newer version,
  - update-check asks before installing a newer version,
  - after install, selected/all running instances are refreshed through the
    newly installed CLI,
  - JSON list output.
- Existing foreground, background, target-scoped status, and target-scoped stop
  tests continue to pass; foreground serve processes are now discoverable while
  they are alive.
- `npm run release:check` passes before release.

## Out of Scope

- Remote process management.
- Killing arbitrary processes not created by Architext.
- A hosted dashboard for background servers.
- Changing the default foreground behavior of `architext serve`.
